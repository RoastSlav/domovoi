//! One real SQLite transaction per incident command: the signal, the incident row, its
//! timeline and the mirrored journal entry land together, or none of them does.

use std::sync::Arc;

use domovoi_core::{CheckKey, HostId, Severity, Signal, SignalClass, SourceKind};
use domovoi_engine::{CommandError, EnginePolicy, IncidentManager, ManagerDeps, PatternRedactor};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::FakeClock;
use domovoi_testkit::build::T0;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::TryRecvError;
use tokio_util::sync::CancellationToken;

async fn rows(store: &Store, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap()
}

fn firing() -> Signal {
    Signal::firing(
        SourceKind::Agent,
        HostId::new(),
        CheckKey::parse("power.supply").unwrap(),
        None,
        Severity::Warning,
        "the power supply failed".to_owned(),
        T0,
    )
    .with_class(SignalClass::Immediate)
}

#[tokio::test]
async fn incident_write_is_atomic_on_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hub.db");
    let store = Arc::new(
        Store::open(&path, StoreOptions::beside(&path, T0))
            .await
            .unwrap(),
    );
    let clock = Arc::new(FakeClock::new(T0));
    let (bus, mut events) = broadcast::channel(64);
    let shutdown = CancellationToken::new();
    let (manager, _task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
        acks: store.clone(),
        clock,
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus,
        shutdown: shutdown.clone(),
    });

    sqlx::query(
        "CREATE TRIGGER fail_events BEFORE INSERT ON incident_events \
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .execute(store.pool_for_tests())
    .await
    .unwrap();

    let signal = firing();
    let refused = manager.ingest(vec![signal.clone()]).await;
    assert!(
        matches!(refused, Err(CommandError::Store(_))),
        "a failing timeline insert fails the command: {refused:?}",
    );
    for (name, sql) in [
        ("signals", "SELECT COUNT(*) FROM signals"),
        ("incidents", "SELECT COUNT(*) FROM incidents"),
        ("incident_signals", "SELECT COUNT(*) FROM incident_signals"),
        ("incident_events", "SELECT COUNT(*) FROM incident_events"),
        ("journal", "SELECT COUNT(*) FROM journal"),
    ] {
        assert_eq!(
            rows(&store, sql).await,
            0,
            "{name} was rolled back with the rest",
        );
    }
    assert!(
        matches!(events.try_recv(), Err(TryRecvError::Empty)),
        "nothing is published for a failed commit",
    );

    sqlx::query("DROP TRIGGER fail_events")
        .execute(store.pool_for_tests())
        .await
        .unwrap();
    manager.ingest(vec![signal]).await.unwrap();
    assert_eq!(
        rows(&store, "SELECT COUNT(*) FROM incidents").await,
        1,
        "one incident opened",
    );
    assert_eq!(
        rows(&store, "SELECT COUNT(*) FROM signals").await,
        1,
        "its signal is stored",
    );
    assert!(
        rows(&store, "SELECT COUNT(*) FROM incident_events").await > 0,
        "its timeline is stored",
    );
    assert!(
        rows(&store, "SELECT COUNT(*) FROM journal").await > 0,
        "the timeline is mirrored into the journal",
    );
    shutdown.cancel();
}
