//! The journal feed: the `incident_events` copy, the pruned floor and the published head.

use std::path::Path;

use domovoi_core::ports::{
    AggregateRef, CommitError, Expected, HostRepo, IncidentRepo, IncidentTimelineEntry,
    JournalDraft, JournalFeed, JournalKind, JournalRead, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{Actor, IncidentId, NoteId};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{at, host, open_incident};
use futures::FutureExt;
use serde_json::{Value, json};

async fn fresh(dir: &Path) -> Store {
    let path = dir.join("db").join("domovoi.db");
    Store::open(&path, StoreOptions::beside(&path, at(1)))
        .await
        .unwrap()
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
async fn journal_mirrors_every_incident_event() {
    let dir = tempfile::tempdir().unwrap();
    let store = fresh(dir.path()).await;
    let host = host("feed-host").unwrap();
    let incident = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    store.upsert_host(&host).await.unwrap();
    store.insert(&incident).await.unwrap();
    for ms in [77, 78] {
        let entry = IncidentTimelineEntry {
            incident: incident.id(),
            at: at(ms),
            kind: "status.changed".to_owned(),
            summary: "s".to_owned(),
            detail: json!({"a": 1}),
        };
        store
            .commit(Transaction::new().write(StateWrite::IncidentEvent(entry)))
            .await
            .unwrap();
    }
    let events: Vec<i64> = sqlx::query_scalar("SELECT seq FROM incident_events")
        .fetch_all(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
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

#[tokio::test]
async fn journal_floor_and_head_start_at_zero_on_a_fresh_database() {
    let dir = tempfile::tempdir().unwrap();
    let store = fresh(dir.path()).await;
    assert_eq!(store.floor_seq().await.unwrap(), 0);
    assert_eq!(store.head_seq().await.unwrap(), 0);
    assert!(journal_rows(&store).await.is_empty());
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
