//! The cap on connections that have not finished the handshake: idle sockets cannot take every
//! file descriptor, and they are dropped after the deadline.

use std::sync::Arc;

use domovoi_core::ports::{AuditLog, Clock};
use domovoi_testkit::log_capture::LogCapture;
use domovoi_testkit::{FakeClock, InMemoryStore, RecordingSignalSink};
use jiff::SignedDuration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use super::handshake::{PREAUTH_LIMITS, UPGRADE_TIMEOUT};
use super::tests::{NoProbes, T0, settings};
use super::{AgentServer, AgentServerDeps};

/// Lets the server's tasks run until `done` holds.
async fn settle(mut done: impl FnMut() -> bool) {
    for _ in 0..100_000 {
        if done() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the server did not reach the expected state");
}

#[tokio::test]
async fn idle_sockets_are_capped_per_address_and_dropped_after_the_deadline() {
    let (logs, _guard) = LogCapture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let audit: Arc<dyn AuditLog> = store.clone();
    let deps = AgentServerDeps {
        clock: clock.clone() as Arc<dyn Clock>,
        hosts: store.clone(),
        facts: store.clone(),
        sink: Arc::new(RecordingSignalSink::new()),
        audit,
        probes: Arc::new(NoProbes),
        hub_host: domovoi_core::HostId::new(),
    };
    let server = AgentServer::new(settings(dir.path()), deps).unwrap();
    let listener = server.bind().await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serving = server.clone();
    let task = tokio::spawn(async move { serving.serve(listener).await });

    // Eight sockets that never speak fill this address's places.
    let mut idle = Vec::new();
    for _ in 0..PREAUTH_LIMITS.per_source {
        idle.push(TcpStream::connect(addr).await.unwrap());
    }
    settle(|| server.inner.preauth.open() == PREAUTH_LIMITS.per_source).await;
    // Let each connection start its deadline before time moves.
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }

    // The ninth is closed at once, without a TLS handshake.
    let mut ninth = TcpStream::connect(addr).await.unwrap();
    let mut byte = [0u8; 1];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), ninth.read(&mut byte))
        .await
        .expect("the ninth connection was left open");
    assert!(
        matches!(read, Ok(0) | Err(_)),
        "the ninth connection was not refused: {read:?}"
    );
    let lines = logs.lines_with("refused an agent connection");
    assert_eq!(lines.len(), 1, "{}", logs.text());
    for field in [
        "WARN",
        "pending_from_peer=8",
        "limit_per_peer=8",
        "limit_total=64",
    ] {
        assert!(lines[0].contains(field), "line lacks {field}: {}", lines[0]);
    }
    assert_eq!(server.inner.preauth.open(), PREAUTH_LIMITS.per_source);

    // Past the deadline the idle sockets are dropped and their places come back.
    clock.advance(UPGRADE_TIMEOUT + SignedDuration::from_secs(1));
    settle(|| server.inner.preauth.open() == 0).await;
    let again = TcpStream::connect(addr).await.unwrap();
    settle(|| server.inner.preauth.open() == 1).await;
    drop((again, idle));
    server.shutdown();
    task.await.unwrap().unwrap();
}
