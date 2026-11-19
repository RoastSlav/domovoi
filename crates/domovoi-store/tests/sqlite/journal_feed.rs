//! The journal feed: the `incident_events` copy, the pruned floor and the published head.

use std::path::{Path, PathBuf};

use domovoi_core::ports::{
    AggregateRef, CommitError, Expected, HostRepo, IncidentRepo, IncidentTimelineEntry,
    JournalDraft, JournalFeed, JournalKind, JournalRead, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{Actor, IncidentId, NoteId};
use domovoi_store::{MIGRATOR, MIN_READER, Store, StoreOptions};
use domovoi_testkit::build::{at, host, open_incident};
use futures::FutureExt;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection, SqliteConnection};

fn copy_fixture(dir: &Path) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("n1-schema5.db");
    let dbdir = dir.join("db");
    std::fs::create_dir_all(&dbdir).unwrap();
    let path = dbdir.join("domovoi.db");
    std::fs::copy(fixture, &path).unwrap();
    path
}

async fn raw(path: &Path) -> SqliteConnection {
    sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .unwrap()
}

async fn open(path: &Path) -> Store {
    Store::open(path, StoreOptions::beside(path, at(1000)))
        .await
        .unwrap()
}

async fn fresh(dir: &Path) -> Store {
    let path = dir.join("db").join("domovoi.db");
    Store::open(&path, StoreOptions::beside(&path, at(1)))
        .await
        .unwrap()
}

async fn an_incident(conn: &mut SqliteConnection) -> String {
    sqlx::query_scalar("SELECT id FROM incidents LIMIT 1")
        .fetch_one(conn)
        .await
        .unwrap()
}

async fn legacy_event(
    conn: &mut SqliteConnection,
    incident: &str,
    ms: i64,
    kind: &str,
    detail: &str,
) {
    sqlx::query(
        "INSERT INTO incident_events (incident_id, at, kind, summary, detail) \
         VALUES (?, ?, ?, 's', ?)",
    )
    .bind(incident)
    .bind(ms)
    .bind(kind)
    .bind(detail)
    .execute(conn)
    .await
    .unwrap();
}

/// `seq, aggregate_type, aggregate_id, kind, payload_v, payload, actor, occurred_at,
/// legacy_event_seq`.
type Copied = (
    i64,
    String,
    String,
    String,
    i64,
    String,
    String,
    i64,
    Option<i64>,
);

async fn journal_rows(store: &Store) -> Vec<Copied> {
    sqlx::query_as(
        "SELECT seq, aggregate_type, aggregate_id, kind, payload_v, payload, actor, occurred_at, \
         legacy_event_seq FROM journal ORDER BY seq",
    )
    .fetch_all(store.pool_for_tests())
    .await
    .unwrap()
}

fn row(incident: IncidentId) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::incident(incident),
        kind: JournalKind::IncidentTimeline,
        payload_v: 1,
        payload: json!({"kind": "k", "summary": "s", "detail": null}),
        actor: Actor::System("test".to_owned()),
        occurred_at: at(5),
    }
}

async fn commit_rows(store: &Store, count: usize) {
    let mut tx = Transaction::new();
    for _ in 0..count {
        tx = tx.journal(row(IncidentId::new()));
    }
    store.commit(tx).await.unwrap();
}

#[tokio::test]
async fn copy_fills_unmirrored_incident_events_from_the_schema5_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let mut conn = raw(&path).await;
    let legacy: Vec<(i64, String, i64, String, String, String)> = sqlx::query_as(
        "SELECT seq, incident_id, at, kind, summary, detail FROM incident_events ORDER BY seq",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    assert!(!legacy.is_empty());
    let store = open(&path).await;
    let rows = journal_rows(&store).await;
    assert_eq!(rows.len(), legacy.len());
    for (seq, incident, at_ms, kind, summary, detail) in legacy {
        let copied = rows.iter().find(|r| r.8 == Some(seq)).unwrap();
        assert_eq!(copied.1, "incident");
        assert_eq!(copied.2, incident);
        assert_eq!(copied.3, "incident.timeline");
        assert_eq!(copied.4, 1);
        assert_eq!(copied.6, "system:incidents");
        assert_eq!(copied.7, at_ms);
        let payload: Value = serde_json::from_str(&copied.5).unwrap();
        let detail: Value = serde_json::from_str(&detail).unwrap();
        assert_eq!(
            payload,
            json!({"kind": kind, "summary": summary, "detail": detail})
        );
    }
}

#[tokio::test]
async fn copy_orders_rows_by_time_then_legacy_seq() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let mut conn = raw(&path).await;
    let incident = an_incident(&mut conn).await;
    sqlx::query("DELETE FROM incident_events")
        .execute(&mut conn)
        .await
        .unwrap();
    for (ms, kind) in [(300, "c"), (100, "a"), (300, "d"), (200, "b")] {
        legacy_event(&mut conn, &incident, ms, kind, "null").await;
    }
    conn.close().await.unwrap();
    let store = open(&path).await;
    let kinds: Vec<String> = journal_rows(&store)
        .await
        .iter()
        .map(|r| {
            let payload: Value = serde_json::from_str(&r.5).unwrap();
            payload["kind"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(kinds, ["a", "b", "c", "d"]);
}

#[tokio::test]
async fn copy_skips_rows_the_dual_write_already_mirrored() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let before_copy = MIGRATOR.iter().map(|m| m.version).max().unwrap() - 1;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path))
        .await
        .unwrap();
    MIGRATOR.run_to(before_copy, &pool).await.unwrap();
    let legacy: Vec<i64> = sqlx::query_scalar("SELECT seq FROM incident_events ORDER BY seq")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(legacy.len() >= 3);
    for (seq, mirrored) in [(40, legacy[0]), (50, legacy[2])] {
        sqlx::query(
            "INSERT INTO journal (seq, aggregate_type, aggregate_id, kind, payload_v, payload, \
             actor, occurred_at, legacy_event_seq) \
             VALUES (?, 'incident', 'x', 'incident.timeline', 1, '{}', 'system:incidents', 1, ?)",
        )
        .bind(seq)
        .bind(mirrored)
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;
    let store = open(&path).await;
    let rows = journal_rows(&store).await;
    assert!(rows.iter().any(|r| r.0 == 40 && r.8 == Some(legacy[0])));
    assert!(rows.iter().any(|r| r.0 == 50 && r.8 == Some(legacy[2])));
    for seq in &legacy {
        assert_eq!(rows.iter().filter(|r| r.8 == Some(*seq)).count(), 1);
    }
    assert_eq!(rows.len(), legacy.len());
}

#[tokio::test]
async fn copy_keeps_unknown_legacy_kinds_and_invalid_detail() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let mut conn = raw(&path).await;
    let incident = an_incident(&mut conn).await;
    legacy_event(&mut conn, &incident, 9, "weird.kind", "not json").await;
    conn.close().await.unwrap();
    let store = open(&path).await;
    let rows = journal_rows(&store).await;
    let weird = rows
        .iter()
        .find(|r| r.5.contains("weird.kind"))
        .expect("the weird row was copied");
    assert_eq!(weird.3, "incident.timeline");
    let payload: Value = serde_json::from_str(&weird.5).unwrap();
    assert_eq!(payload["kind"], "weird.kind");
    assert_eq!(payload["detail"], "not json");
}

#[tokio::test]
async fn reopening_after_the_copy_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let first = journal_rows(&open(&path).await).await;
    let second = journal_rows(&open(&path).await).await;
    assert_eq!(first, second);
}

#[tokio::test]
async fn journal_mirrors_every_incident_event_after_the_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let store = open(&path).await;
    let host = host("feed-host").unwrap();
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    store.upsert_host(&host).await.unwrap();
    store.insert(&incident).await.unwrap();
    let entry = IncidentTimelineEntry {
        incident: incident.id(),
        at: at(77),
        kind: "status.changed".to_owned(),
        summary: "s".to_owned(),
        detail: json!({"a": 1}),
    };
    store
        .commit(Transaction::new().write(StateWrite::IncidentEvent(entry)))
        .await
        .unwrap();
    let events: Vec<i64> = sqlx::query_scalar("SELECT seq FROM incident_events")
        .fetch_all(store.pool_for_tests())
        .await
        .unwrap();
    let rows = journal_rows(&store).await;
    for seq in &events {
        assert_eq!(rows.iter().filter(|r| r.8 == Some(*seq)).count(), 1);
    }
    let keys = |r: &Copied| -> Vec<String> {
        let payload: Value = serde_json::from_str(&r.5).unwrap();
        payload.as_object().unwrap().keys().cloned().collect()
    };
    let mirrored = rows.last().unwrap();
    assert_eq!(mirrored.3, "incident.timeline");
    assert_eq!(keys(mirrored), keys(&rows[0]));
}

#[test]
fn journal_feed_migration_is_additive() {
    let sql = include_str!("../../migrations/0013_journal_feed.sql").to_uppercase();
    for forbidden in ["DROP", "ALTER", "DELETE"] {
        assert!(!sql.contains(forbidden), "{forbidden}");
    }
    assert_eq!(MIN_READER, "0.0.0");
}

#[tokio::test]
async fn journal_floor_starts_at_zero_after_the_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy_fixture(dir.path());
    let store = open(&path).await;
    let highest = journal_rows(&store).await.last().unwrap().0;
    assert_eq!(store.floor_seq().await.unwrap(), 0);
    assert_eq!(store.head_seq().await.unwrap(), highest as u64);
}

#[tokio::test]
async fn prune_raises_the_floor_and_the_head_never_moves_backwards() {
    let dir = tempfile::tempdir().unwrap();
    let store = fresh(dir.path()).await;
    commit_rows(&store, 5).await;
    assert_eq!(store.prune_journal_through(5).await.unwrap(), 5);
    assert_eq!(store.floor_seq().await.unwrap(), 5);
    assert_eq!(store.head_seq().await.unwrap(), 5);
    assert!(store.entries_after(0, 10).await.unwrap().is_empty());
    commit_rows(&store, 1).await;
    assert_eq!(store.entries_after(0, 10).await.unwrap()[0].seq, 6);
}

#[tokio::test]
async fn prune_never_lowers_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let store = fresh(dir.path()).await;
    commit_rows(&store, 5).await;
    store.prune_journal_through(5).await.unwrap();
    store.prune_journal_through(3).await.unwrap();
    assert_eq!(store.floor_seq().await.unwrap(), 5);
}

#[tokio::test]
async fn commit_publishes_the_head_after_the_commit_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = fresh(dir.path()).await;
    let head = store.head_seq().await.unwrap();
    assert!(store.wait_after(head).now_or_never().is_none());
    commit_rows(&store, 1).await;
    assert!(store.wait_after(head).now_or_never().is_some());

    let head = store.head_seq().await.unwrap();
    let host = host("feed-host").unwrap();
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    store.upsert_host(&host).await.unwrap();
    store.insert(&incident).await.unwrap();
    let failed = store
        .commit(
            Transaction::new()
                .journal(JournalDraft {
                    aggregate: AggregateRef::note(NoteId::new()),
                    ..row(IncidentId::new())
                })
                .write(StateWrite::Incident {
                    incident,
                    expected: Expected::New,
                }),
        )
        .await;
    assert!(matches!(failed, Err(CommitError::Conflict { .. })));
    assert!(store.wait_after(head).now_or_never().is_none());
}
