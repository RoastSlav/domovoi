//! The spool keeps batches on disk until acknowledged and replays them after a reconnect.

use std::sync::Arc;

use domovoi_agent::spool::{MAX_BYTES, SharedSpool, Spool};
use domovoi_core::{HostId, Severity, Signal, SignalStatus, SourceKind};
use domovoi_proto::Message;
use domovoi_proto::messages::{Ping, SignalsAck};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use crate::harness::{clock, identity, key, start_link_with, target};
use crate::hub::{TestHub, guard};
use crate::link::advancing;

fn signal(name: &str, severity: Severity) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        HostId::from_uuid(uuid::Uuid::nil()),
        key("test.check"),
        target(name),
        severity,
        format!("{name} fired"),
        T0,
    )
}

fn resolved(name: &str) -> Signal {
    Signal::resolved(
        SourceKind::Agent,
        HostId::from_uuid(uuid::Uuid::nil()),
        key("test.check"),
        target(name),
        format!("{name} recovered"),
        T0,
    )
}

fn targets(spool: &Spool) -> Vec<Vec<String>> {
    spool
        .pending_after(None)
        .iter()
        .map(|b| {
            b.signals
                .iter()
                .map(|s| s.target().map_or(String::new(), |t| t.to_string()))
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn signals_are_spooled_and_replayed_in_order_after_reconnect() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    // Spooled while the hub was out of reach.
    spool
        .push(vec![signal("a", Severity::Warning)])
        .await
        .unwrap();
    spool
        .push(vec![signal("b", Severity::Critical)])
        .await
        .unwrap();
    let link = start_link_with(&hub, &identity, &clock, &dir, spool.clone());

    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;
    assert_eq!(conn.signals().await.batch_seq, 0);
    assert_eq!(conn.signals().await.batch_seq, 1);
    // Unacknowledged, so a new connection gets both again, in order.
    drop(conn);
    let (mut conn, _) = advancing(&clock, SignedDuration::from_millis(100), hub.accept()).await;
    conn.welcome(&identity, 3600).await;
    let first = conn.signals().await;
    assert_eq!(first.batch_seq, 0);
    assert_eq!(first.signals[0].target().unwrap().as_str(), "a");
    assert_eq!(conn.signals().await.batch_seq, 1);

    // New batches go out while connected.
    spool
        .push(vec![signal("c", Severity::Warning)])
        .await
        .unwrap();
    assert_eq!(conn.signals().await.batch_seq, 2);

    conn.send(Message::SignalsAck(SignalsAck { up_to_seq: 2 }), None)
        .await;
    conn.send(Message::Ping(Ping { at: T0 }), None).await;
    let pong = conn.expect().await;
    assert!(matches!(pong.message, Message::Pong(_)));
    assert!(spool.lock().await.is_empty());
    link.shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[test]
fn ack_truncates_spool() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    for name in ["a", "b", "c"] {
        spool.push(vec![signal(name, Severity::Warning)]).unwrap();
    }
    spool.ack(1).unwrap();
    assert_eq!(targets(&spool), [vec!["c".to_owned()]]);
    // What is on disk matches.
    let reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [vec!["c".to_owned()]]);
    assert_eq!(reopened.pending_after(None)[0].batch_seq, 2);
    // Acknowledging an old sequence again changes nothing.
    let mut reopened = reopened;
    reopened.ack(0).unwrap();
    assert_eq!(reopened.len(), 1);
}

#[test]
fn full_spool_drops_info_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap().with_limits(3, MAX_BYTES);
    spool
        .push(vec![
            signal("info-1", Severity::Info),
            signal("warn-1", Severity::Warning),
        ])
        .unwrap();
    spool.push(vec![signal("info-2", Severity::Info)]).unwrap();
    spool.push(vec![resolved("gone")]).unwrap();
    spool
        .push(vec![signal("crit", Severity::Critical)])
        .unwrap();
    // Four batches over a cap of three: info signals go first, oldest first. The first
    // batch keeps its warning; the second, now empty, goes. Resolved signals stay.
    assert_eq!(
        targets(&spool),
        vec![
            vec!["warn-1".to_owned()],
            vec!["gone".to_owned()],
            vec!["crit".to_owned()],
        ]
    );
    assert_eq!(spool.dropped(), 2);

    // With no info left, whole oldest batches go.
    spool
        .push(vec![signal("crit-2", Severity::Critical)])
        .unwrap();
    assert_eq!(targets(&spool).concat(), ["gone", "crit", "crit-2"]);
    assert_eq!(spool.dropped(), 3);
    assert_eq!(Spool::open(dir.path()).unwrap().dropped(), 3);
}

#[test]
fn batch_seq_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    spool.push(vec![signal("a", Severity::Warning)]).unwrap();
    spool.push(vec![signal("b", Severity::Warning)]).unwrap();
    spool.ack(1).unwrap();
    assert!(spool.is_empty());
    drop(spool);
    let mut spool = Spool::open(dir.path()).unwrap();
    assert_eq!(spool.push(vec![signal("c", Severity::Warning)]).unwrap(), 2);
    let pending = spool.pending_after(None);
    assert_eq!(pending[0].signals[0].status(), SignalStatus::Firing);
    #[cfg(unix)]
    for file in ["spool.jsonl", "spool.json"] {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(file))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "{file}");
    }
}
