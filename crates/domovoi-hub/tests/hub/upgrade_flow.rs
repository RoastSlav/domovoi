//! The end-to-end upgrade: a database written before the rework opens at the current
//! schema, the services run against it, new writes are chained and journaled, the audit
//! history still verifies, and the invariants hold after a restart.
//!
//! The fixture is never modified: every test copies it into a temporary directory first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use domovoi_core::audit_chain::{ChainStatus, VerifyScope};
use domovoi_core::ports::{
    AggregateRef, IncidentFilter, IncidentRepo, JournalEntry, JournalRead, Page, VersionedRead,
};
use domovoi_core::{
    CheckKey, HostId, IncidentId, Severity, Signal, SignalClass, SourceKind, UserId,
};
use domovoi_engine::{EnginePolicy, IncidentManager, ManagerDeps, ManagerHandle, PatternRedactor};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::FakeClock;
use domovoi_testkit::build::T0;
use domovoi_testkit::crash::{Exempt, check_invariants_except};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::sqlite::SqliteStore;

/// The committed database written before the rework; it is never regenerated.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../domovoi-store/tests/fixtures/n1-schema5.db")
}

/// Copies the fixture into `dir` and returns the copy's path.
fn copy_fixture(dir: &Path) -> PathBuf {
    let dbdir = dir.join("db");
    std::fs::create_dir_all(&dbdir).expect("the temporary database directory");
    let path = dbdir.join("domovoi.db");
    std::fs::copy(fixture(), &path).expect("the fixture is copied");
    path
}

/// The fixture's copy, opened at the current schema.
async fn upgraded(dir: &Path) -> (Arc<SqliteStore>, PathBuf) {
    let path = copy_fixture(dir);
    let store = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .expect("the upgraded database opens");
    (Arc::new(SqliteStore::new(store)), path)
}

/// The fixture's row counts, in the order the upgrade test checks them.
const FIXTURE_ROWS: [(&str, i64); 10] = [
    ("hosts", 2),
    ("agents", 1),
    ("incidents", 3),
    ("signals", 3),
    ("incident_signals", 3),
    ("incident_events", 4),
    ("actions", 2),
    ("audit_log", 5),
    ("notes", 2),
    ("settings", 1),
];

/// U1: the schema upgrade keeps every row, starts every version at 1, and leaves a sound
/// database behind.
#[tokio::test]
async fn upgraded_database_opens_and_keeps_its_content() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;

    store
        .quick_check()
        .await
        .expect("the upgraded database is sound");
    for (table, expected) in FIXTURE_ROWS {
        let sql = format!("SELECT count(*) FROM {table}");
        let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_one(store.pool_for_tests())
            .await
            .expect("the table is readable");
        assert_eq!(count, expected, "{table} kept its rows");
    }

    let incidents = incidents(&store).await;
    assert_eq!(incidents.len(), 3);
    for incident in &incidents {
        let read = VersionedRead::incident(&*store, incident.id())
            .await
            .expect("the incident is readable")
            .expect("every fixture incident still loads");
        assert_eq!(read.version, 1, "the upgrade starts every incident at 1");
    }
}

/// U2: the manager writes an upgraded incident through its stored version and mirrors the
/// timeline into the journal.
#[tokio::test]
async fn manager_updates_an_upgraded_incident_with_its_stored_version() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;
    let incident = open_incident(&store).await;
    let timeline_before = IncidentRepo::timeline(&*store, incident.id())
        .await
        .expect("the timeline is readable")
        .len();
    let journal_before = timeline_journal(&store, incident.id()).await.len();

    let harness = Harness::start(store.clone());
    harness
        .manager
        .ack(incident.id(), UserId::new())
        .await
        .expect("an incident stored before the rework can be acked");

    let read = VersionedRead::incident(&*store, incident.id())
        .await
        .expect("the incident is readable")
        .expect("the incident is still there");
    assert_eq!(read.version, 2, "the ack advanced the stored version");
    let timeline = IncidentRepo::timeline(&*store, incident.id())
        .await
        .expect("the timeline is readable");
    assert_eq!(
        timeline.len(),
        timeline_before + 2,
        "the ack adds its human line and the status change"
    );
    let journal = timeline_journal(&store, incident.id()).await;
    assert_eq!(
        journal.len(),
        journal_before + 2,
        "each new timeline entry is mirrored into the journal"
    );
    harness.stop().await;
}

/// U3: a signal for a fingerprint the fixture never had opens an incident and journals it.
#[tokio::test]
async fn new_incident_after_upgrade_is_stored_and_journaled() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;
    let host = incidents(&store)
        .await
        .first()
        .expect("the fixture has incidents")
        .host();
    let harness = Harness::start(store.clone());

    harness
        .manager
        .ingest(vec![firing(host, "net.link")])
        .await
        .expect("a new fingerprint opens an incident");

    let created = incidents(&store)
        .await
        .into_iter()
        .find(|incident| incident.check().as_str() == "net.link")
        .expect("the new incident is stored");
    let journal = journal_for(&store, AggregateRef::incident(created.id())).await;
    assert!(
        journal
            .iter()
            .all(|entry| entry.kind == "incident.timeline"),
        "the new incident's timeline is journaled: {journal:#?}"
    );
    assert!(
        !journal.is_empty(),
        "the new incident has its timeline in the journal"
    );
    harness.stop().await;
}

/// U5: anything written after the upgrade is a chain.
#[tokio::test]
async fn audit_verifies_after_upgrade() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;

    let report = store
        .verify_audit(VerifyScope::Full)
        .await
        .expect("the audit history is readable");
    assert!(
        matches!(
            report.chained,
            ChainStatus::Empty | ChainStatus::Intact { .. }
        ),
        "the rows written after the upgrade are chained: {:?}",
        report.chained
    );
}

/// U6: the invariants hold on the upgraded fixture, after a new write, and after a restart.
#[tokio::test]
async fn invariants_hold_after_upgrade_and_a_restart() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, path) = upgraded(dir.path()).await;
    let exempt = legacy();
    check_invariants_except(&*store, &exempt)
        .await
        .expect("the upgraded fixture alone satisfies the invariants");

    let harness = Harness::start(store.clone());
    let incident = open_incident(&store).await;
    harness
        .manager
        .ack(incident.id(), UserId::new())
        .await
        .expect("the incident is stored");
    check_invariants_except(&*store, &exempt)
        .await
        .expect("the invariants hold after a new write");
    harness.stop().await;

    store.close().await;
    let reopened = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .expect("the upgraded database reopens");
    let reopened = SqliteStore::new(reopened);
    check_invariants_except(&reopened, &exempt)
        .await
        .expect("the invariants hold after a restart");
}

/// The committed fixture is never written: opening a copy leaves the original's length and
/// digest exactly as they were.
#[tokio::test]
async fn fixture_file_is_not_modified() {
    let original = fixture();
    let before = std::fs::metadata(&original).expect("the fixture exists");
    let digest_before = digest(&original);

    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;
    let incident = open_incident(&store).await;
    let harness = Harness::start(store.clone());
    harness
        .manager
        .ack(incident.id(), UserId::new())
        .await
        .expect("the copy accepts writes");
    harness.stop().await;
    store.close().await;

    let after = std::fs::metadata(&original).expect("the fixture is still there");
    assert_eq!(
        before.len(),
        after.len(),
        "the fixture's length is unchanged"
    );
    assert_eq!(
        digest_before,
        digest(&original),
        "the fixture's digest is unchanged"
    );
}

/// The SHA-256 of a file, as lowercase hex.
fn digest(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the file is readable");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Every stored incident.
async fn incidents(store: &Store) -> Vec<domovoi_core::Incident> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = IncidentRepo::list(store, &IncidentFilter::default(), &page)
            .await
            .expect("the incidents are readable");
        items.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return items,
        }
    }
}

/// The fixture's one open incident.
async fn open_incident(store: &Store) -> domovoi_core::Incident {
    incidents(store)
        .await
        .into_iter()
        .find(|incident| incident.status().name() == "open")
        .expect("the fixture has an open incident")
}

/// A firing signal for `check` on `host`, from the agent.
fn firing(host: HostId, check: &str) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).expect("a valid check key"),
        None,
        Severity::Warning,
        format!("{check} firing"),
        T0,
    )
    .with_class(SignalClass::Immediate)
}

/// The journal rows of `aggregate`, oldest first.
async fn journal_for(store: &Store, aggregate: AggregateRef) -> Vec<JournalEntry> {
    JournalRead::entries_for(store, &aggregate, Page::MAX_LIMIT)
        .await
        .expect("the journal is readable")
}

/// The mirrored timeline journal rows of `incident`.
async fn timeline_journal(store: &Store, incident: IncidentId) -> Vec<JournalEntry> {
    journal_for(store, AggregateRef::incident(incident))
        .await
        .into_iter()
        .filter(|entry| entry.kind == "incident.timeline")
        .collect()
}

/// What the fixture wrote before the journal existed, so the invariants can allow for it. The
/// upgrade copies every legacy timeline row into the journal, so no timeline entry is exempt.
fn legacy() -> Exempt {
    Exempt {
        legacy_timeline: BTreeMap::new(),
    }
}

/// The incident manager over an upgraded store.
struct Harness {
    manager: ManagerHandle,
    shutdown: CancellationToken,
    manager_task: Option<JoinHandle<()>>,
}

impl Harness {
    fn start(store: Arc<SqliteStore>) -> Self {
        let clock = Arc::new(FakeClock::new(T0));
        let (bus, _events) = broadcast::channel(256);
        let shutdown = CancellationToken::new();
        let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
            incidents: store.clone(),
            uow: store.clone(),
            versions: store.clone(),
            acks: store.clone(),
            clock,
            redactor: Arc::new(PatternRedactor::default()),
            policy: EnginePolicy {
                tick_every: jiff::SignedDuration::from_hours(24),
                ..EnginePolicy::default()
            },
            bus,
            shutdown: shutdown.clone(),
        });
        Self {
            manager,
            shutdown,
            manager_task: Some(manager_task),
        }
    }

    async fn stop(mut self) {
        self.shutdown.cancel();
        join(self.manager_task.take()).await;
    }
}

/// Waits for a service task, bounded so a stuck task fails the test instead of hanging.
///
/// This awaits the task rather than spinning on yields: a loaded runner can keep the task
/// from being scheduled within a fixed number of yields even though it is about to stop.
async fn join(task: Option<JoinHandle<()>>) {
    let Some(task) = task else {
        return;
    };
    match tokio::time::timeout(std::time::Duration::from_secs(10), task).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => panic!("a service task failed: {err}"),
        Err(_) => panic!("services did not stop"),
    }
}
