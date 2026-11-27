//! Package transfers over the link: the receiver's answers and the heartbeat while bytes flow.

use std::sync::Arc;

use domovoi_agent::link::IncomingTransfer;
use domovoi_agent::link::frame;
use domovoi_agent::packages::{Staging, TracingEvents, TransferReceiver};
use domovoi_core::RequestId;
use domovoi_core::ports::Clock as _;
use domovoi_proto::messages::{
    Ping, TransferBegin, TransferChunk, TransferEnd, TransferFailureCode, TransferKind,
    TransferOutcome,
};
use domovoi_proto::{Base64Bytes, Message, Open, Sha256Bytes};
use sha2::{Digest as _, Sha256};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::harness::{clock, identity, start_link, start_link_packages, start_link_transfers};
use crate::hub::{HubConn, TestHub, guard};
use crate::package_rig::{Options, Rig};

fn begin(id: Uuid, bytes: &[u8]) -> Message {
    Message::TransferBegin(TransferBegin {
        transfer_id: id,
        kind: Open::new("package_artifact", Some(TransferKind::PackageArtifact)),
        size: bytes.len() as u64,
        sha256: Sha256Bytes(Sha256::digest(bytes).into()),
    })
}

fn chunk(id: Uuid, seq: u64, bytes: &[u8]) -> Message {
    Message::TransferChunk(TransferChunk {
        transfer_id: id,
        seq,
        bytes: Base64Bytes(bytes.to_vec()),
    })
}

/// Sends `message` as the hub and returns the envelope id the agent must reply to.
async fn send(conn: &mut HubConn, message: Message) -> RequestId {
    frame::send(conn.transport.as_mut(), &message, None)
        .await
        .unwrap()
}

#[tokio::test]
async fn transfer_end_without_section_is_answered_aborted() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;

    let id = Uuid::now_v7();
    send(&mut conn, begin(id, b"abcd")).await;
    send(&mut conn, chunk(id, 0, b"abcd")).await;
    let end = send(
        &mut conn,
        Message::TransferEnd(TransferEnd { transfer_id: id }),
    )
    .await;

    let answer = guard(conn.expect()).await;
    assert_eq!(answer.reply_to, Some(end));
    let Message::TransferResult(result) = answer.message else {
        panic!("expected a transfer result, got {:?}", answer.message);
    };
    assert_eq!(result.transfer_id, id);
    let TransferOutcome::Failed { code, .. } = result.outcome else {
        panic!("a transfer must fail without a [packages] section");
    };
    assert_eq!(code.known(), Some(&TransferFailureCode::Aborted));
    link.shutdown.cancel();
}

#[tokio::test]
async fn heartbeat_is_answered_while_chunks_flow() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let staging = Staging::open(dir.path(), clock.clone()).unwrap();
    let receiver = TransferReceiver::new(
        staging,
        true,
        1024 * 1024,
        clock.clone(),
        Arc::new(TracingEvents),
    );
    let (frames, incoming) = mpsc::channel::<IncomingTransfer>(64);
    let link = start_link_transfers(&hub, &identity, &clock, &dir, frames);
    let served = tokio::spawn(receiver.serve(incoming, link.handle.clone(), link.shutdown.clone()));
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;

    let bytes = [3_u8; 12];
    let id = Uuid::now_v7();
    send(&mut conn, begin(id, &bytes)).await;
    for seq in 0..2_u64 {
        let start = usize::try_from(seq).unwrap() * 4;
        send(&mut conn, chunk(id, seq, &bytes[start..start + 4])).await;
    }
    send(&mut conn, Message::Ping(Ping { at: clock.now() })).await;
    let pong = guard(conn.expect()).await;
    assert!(
        matches!(pong.message, Message::Pong(_)),
        "a ping between chunks must be answered first: {pong:?}"
    );
    send(&mut conn, chunk(id, 2, &bytes[8..])).await;
    let end = send(
        &mut conn,
        Message::TransferEnd(TransferEnd { transfer_id: id }),
    )
    .await;

    let answer = guard(conn.expect()).await;
    assert_eq!(answer.reply_to, Some(end));
    let Message::TransferResult(result) = answer.message else {
        panic!("expected a transfer result, got {:?}", answer.message);
    };
    assert_eq!(result.outcome, TransferOutcome::Stored);
    link.shutdown.cancel();
    served.await.unwrap();
}

/// 48b: the catalogue hash in `hello` describes the list the agent pages now, enabled
/// package skills included, not the one it had when it started.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn hello_announces_the_catalogue_with_the_enabled_package_skill() {
    use domovoi_agent::requests::catalog_entries_with;
    use domovoi_core::capability::catalog_hash;
    use domovoi_testkit::PackageFixture;

    let built = PackageFixture::new("nginx-tools", "1.2.0")
        .unwrap()
        .skill("nginx_reload")
        .build();
    let rig = Rig::new(&[&built], Options::default());
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    rig.call(rig.run("agent.package.install", &args)).await;
    let stale = rig.ctx.package_skills_surface();
    let stale = catalog_entries_with(&rig.ctx.catalog, stale, &[]);
    let pair = [
        ("package", "nginx-tools".to_owned()),
        ("version", "1.2.0".to_owned()),
    ];
    rig.call(rig.run("agent.package.enable", &pair)).await;
    let now = catalog_entries_with(
        &rig.ctx.catalog,
        rig.ctx.package_skills_surface(),
        &rig.ctx.enabled_package_skill_names(),
    );
    assert_ne!(catalog_hash(&stale), catalog_hash(&now));

    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let old = domovoi_agent::link::handshake::catalog_announcement(&stale).unwrap();
    let _link = start_link_packages(&hub, &identity, &clock, &dir, Arc::new(rig.ctx), old);
    let hello = hub.accept().await.welcome(&identity, 3600).await;
    assert_eq!(
        hello.catalog_hash.map(|hash| hex::encode(hash.as_bytes())),
        Some(catalog_hash(&now))
    );
    assert!(hello.package_skills[0].enabled);
}
