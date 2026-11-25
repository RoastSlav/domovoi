//! `safety.attest` and `safety.public_key` through the connection server.

use domovoi_config::parse_helper;
use domovoi_core::attest::{Hash32, Report, parse_report};
use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperRequest, HelperRequestFrame, HelperResponse, Nonce, decode_response,
    encode_frame,
};
use domovoi_helper::attest::keys::load_key;
use domovoi_helper::serve::{ServeContext, ServeOutcome, serve_connection};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use uuid::Uuid;

use crate::conn::TestConn;
use crate::world::{AGENT_ID, World, verify};

fn context(world: &World) -> ServeContext {
    let config = parse_helper(&world.text.helper_toml).map(|(config, _)| config);
    ServeContext::new(
        config,
        world.inspect(),
        world.clock.clone(),
        world.dirs.clone(),
        "test-build",
    )
    .with_process_executor(world.exec.clone())
}

async fn ask(ctx: &ServeContext, request: HelperRequest) -> HelperResponse {
    let frame = HelperRequestFrame {
        v: HELPER_PROTOCOL,
        id: Uuid::now_v7(),
        request,
    };
    let (mut client, server) = UnixStream::pair().expect("pair");
    client
        .write_all(&encode_frame(&frame).expect("encode"))
        .await
        .expect("send");
    let outcome = serve_connection(
        TestConn {
            stream: server,
            uid: AGENT_ID,
        },
        ctx,
    )
    .await;
    assert_eq!(outcome, ServeOutcome::Answered);
    let mut answer = Vec::new();
    client.read_to_end(&mut answer).await.expect("read answer");
    decode_response(&answer[4..]).expect("decodes").response
}

fn world() -> World {
    let mut world = World::new();
    world.text.helper_toml =
        "groups = [\"safety.attest\"]\n[safety]\nmanagement_cidrs = [\"198.51.100.0/24\"]\n"
            .to_owned();
    world.init();
    world
}

#[tokio::test]
async fn the_attest_op_answers_a_signed_report_bound_to_the_nonce() {
    let world = world();
    let ctx = context(&world);
    let HelperResponse::SafetyAttested { attestation } = ask(
        &ctx,
        HelperRequest::SafetyAttest {
            nonce: Nonce::from_bytes([9; 32]),
        },
    )
    .await
    else {
        panic!("expected a signed attestation");
    };
    let Report::Complete(report) = verify(&attestation) else {
        panic!("complete");
    };
    assert_eq!(report.nonce.as_bytes(), &[9; 32]);
    assert_eq!(report.generation, 1);
    assert!(parse_report(&attestation.report).is_ok());
}

#[tokio::test]
async fn the_public_key_op_returns_only_the_public_key() {
    let world = world();
    let ctx = context(&world);
    let HelperResponse::SafetyPublicKey { public_key } =
        ask(&ctx, HelperRequest::SafetyPublicKey).await
    else {
        panic!("expected the public key");
    };
    let key = load_key(world.dir()).expect("key");
    assert_eq!(public_key, Hash32::from_bytes(key.public_key()).to_string());
    let seed = std::fs::read(world.path("attest.key")).expect("seed");
    assert!(
        !public_key.contains(&seed.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        "the seed never leaves the helper"
    );
}
