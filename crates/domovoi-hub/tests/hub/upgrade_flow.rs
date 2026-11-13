//! The end-to-end upgrade: a database written before the rework opens at the current
//! schema, the services run against it, new writes are chained and journaled, the audit
//! history still verifies, and the invariants hold after a restart.
//!
//! The fixture is never modified: every test copies it into a temporary directory first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use domovoi_core::audit_chain::{ChainStatus, LegacyStatus, VerifyScope};
use domovoi_core::ports::{
    ActionFilter, ActionRepo, AggregateRef, AuditFilter, AuditLog, IncidentFilter, IncidentRepo,
    JournalEntry, JournalRead, Page, VersionedRead,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, CheckKey, HostId, IncidentId, Severity, Signal,
    SignalClass, SourceKind, UserId,
};
use domovoi_engine::{
    ActionDeps, ActionHandle, ActionService, ActionSettings, EnginePolicy, IncidentManager,
    ManagerDeps, ManagerHandle, PatternRedactor,
};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use domovoi_testkit::crash::{Exempt, check_invariants_except};
use domovoi_testkit::{FakeAgentLink, FakeClock};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
async fn upgraded(dir: &Path) -> (Arc<Store>, PathBuf) {
    let path = copy_fixture(dir);
    let store = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .expect("the upgraded database opens");
    (Arc::new(store), path)
}

/// The fixture's row counts, in the order the upgrade test checks them.
const FIXTURE_ROWS: [(&str, i64); 9] = [
    ("hosts", 2),
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

/// U4: the fixture's proposal is approved through the writer, audited as the first row of
/// the chain, and journaled; the fixture's approved action is then refused by the executor
/// because its host announces no skills.
#[tokio::test]
async fn legacy_action_can_be_approved_after_upgrade() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;
    let actions = actions(&store).await;
    let proposed = actions
        .iter()
        .find(|action| action.status() == &ActionStatus::Proposed)
        .expect("the fixture has a proposal");
    let approved_before = actions
        .iter()
        .find(|action| action.status() == &ActionStatus::Approved)
        .expect("the fixture has an approved action");
    let anchor = store
        .audit_anchor()
        .await
        .expect("the anchor is readable")
        .expect("the upgrade wrote an anchor");

    let harness = Harness::start(store.clone());
    let approved = harness
        .actions
        .approve(proposed.id(), human(UserId::new()), None)
        .await
        .expect("a proposal stored before the rework can be approved");
    assert_eq!(approved.status(), &ActionStatus::Approved);

    let rows = audit(&store, "action.approved", proposed.id()).await;
    assert_eq!(rows.len(), 1, "the approval is audited once");
    let seq = rows[0];
    let prev: Vec<u8> = sqlx::query_scalar("SELECT prev_hash FROM audit_log WHERE seq = ?")
        .bind(i64::try_from(seq).expect("a sequence number fits"))
        .fetch_one(store.pool_for_tests())
        .await
        .expect("the row has its chain fields");
    let expected = if seq == anchor.legacy_last_id + 1 {
        anchor.hash().to_vec()
    } else {
        sqlx::query_scalar("SELECT hash FROM audit_log WHERE seq = ?")
            .bind(i64::try_from(seq - 1).expect("a sequence number fits"))
            .fetch_one(store.pool_for_tests())
            .await
            .expect("the row before it has its chain fields")
    };
    assert_eq!(
        prev, expected,
        "the approval chains onto the row before it, or onto the anchor when it is first"
    );

    let transitions = journal_for(&store, AggregateRef::action(proposed.id())).await;
    let approval = transitions
        .iter()
        .filter(|entry| {
            entry.kind == "action.transition"
                && entry.payload.get("from").and_then(|from| from.as_str()) == Some("proposed")
        })
        .count();
    assert_eq!(approval, 1, "the approval is journaled from proposed");

    let refused = terminal(&store, approved_before.id()).await;
    assert!(
        matches!(refused.status(), ActionStatus::Rejected { .. }),
        "the host announces no skills, so the executor rejects the legacy approval: {:?}",
        refused.status()
    );
    let kinds = audit_kinds(&store, approved_before.id()).await;
    assert!(
        kinds.iter().any(|kind| kind == "action.rejected"),
        "the refusal is audited: {kinds:?}"
    );
    harness.stop().await;
}

/// U5: the legacy history still matches its anchor and everything written afterwards is a
/// chain.
#[tokio::test]
async fn audit_verifies_after_upgrade() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, _path) = upgraded(dir.path()).await;
    let harness = Harness::start(store.clone());
    let proposed = actions(&store)
        .await
        .into_iter()
        .find(|action| action.status() == &ActionStatus::Proposed)
        .expect("the fixture has a proposal");
    harness
        .actions
        .approve(proposed.id(), human(UserId::new()), None)
        .await
        .expect("the approval is stored");
    harness.stop().await;

    let report = store
        .verify_audit(VerifyScope::Full)
        .await
        .expect("the audit history is readable");
    assert!(
        matches!(report.legacy, LegacyStatus::Verified { count: 5 }),
        "the five rows written before the chain still verify: {:?}",
        report.legacy
    );
    assert!(
        matches!(report.chained, ChainStatus::Intact { .. }),
        "the rows written after the upgrade are chained: {:?}",
        report.chained
    );
}

/// U6: the invariants hold on the upgraded fixture, after a new write, and after a restart.
#[tokio::test]
async fn invariants_hold_after_upgrade_and_a_restart() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (store, path) = upgraded(dir.path()).await;
    let exempt = legacy(&store).await;
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

/// Every stored action.
async fn actions(store: &Store) -> Vec<ActionRequest> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = ActionRepo::list(store, &ActionFilter::default(), &page)
            .await
            .expect("the actions are readable");
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

/// The sequence numbers of the audit rows of `kind` about `action`, oldest first.
async fn audit(store: &Store, kind: &str, action: ActionId) -> Vec<u64> {
    sqlx::query_scalar::<_, i64>(
        "SELECT seq FROM audit_log WHERE kind = ? AND subject = ? ORDER BY seq",
    )
    .bind(kind)
    .bind(format!("action:{action}"))
    .fetch_all(store.pool_for_tests())
    .await
    .expect("the audit rows are readable")
    .into_iter()
    .map(|seq| u64::try_from(seq).expect("a sequence number is positive"))
    .collect()
}

/// Every audit kind recorded for `action`.
async fn audit_kinds(store: &Store, action: ActionId) -> Vec<String> {
    AuditLog::list(
        store,
        &AuditFilter {
            subject: Some(format!("action:{action}")),
            ..AuditFilter::default()
        },
        &Page::first(Page::MAX_LIMIT),
    )
    .await
    .expect("the audit log is readable")
    .items
    .into_iter()
    .map(|event| event.kind)
    .collect()
}

/// Polls the store until `action` has an outcome. The executor's start-up scan needs no
/// clock, so plain yields are enough.
async fn terminal(store: &Store, action: ActionId) -> ActionRequest {
    for _ in 0..10_000 {
        let found = ActionRepo::get(store, action)
            .await
            .expect("the action is readable")
            .expect("the action is still there");
        if !matches!(
            found.status(),
            ActionStatus::Proposed | ActionStatus::Approved | ActionStatus::Dispatched
        ) {
            return found;
        }
        tokio::task::yield_now().await;
    }
    panic!("action {action} never reached an outcome");
}

/// What the fixture wrote before the journal existed, so the invariants can allow for it.
async fn legacy(store: &Store) -> Exempt {
    let actions: BTreeSet<ActionId> = actions(store).await.iter().map(ActionRequest::id).collect();
    let mut timeline = BTreeMap::new();
    for incident in incidents(store).await {
        let entries = IncidentRepo::timeline(store, incident.id())
            .await
            .expect("the timeline is readable");
        timeline.insert(incident.id(), entries.len());
    }
    Exempt {
        actions,
        legacy_timeline: timeline,
    }
}

/// The manager and the action service over an upgraded store, with no agent connected and no
/// skills announced, so every run is refused before it dispatches.
struct Harness {
    manager: ManagerHandle,
    actions: ActionHandle,
    shutdown: CancellationToken,
    manager_task: Option<JoinHandle<()>>,
    action_task: Option<JoinHandle<()>>,
}

impl Harness {
    fn start(store: Arc<Store>) -> Self {
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let (bus, _events) = broadcast::channel(256);
        let shutdown = CancellationToken::new();
        let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
            incidents: store.clone(),
            uow: store.clone(),
            versions: store.clone(),
            acks: store.clone(),
            clock: clock.clone(),
            redactor: Arc::new(PatternRedactor::default()),
            policy: EnginePolicy {
                tick_every: jiff::SignedDuration::from_hours(24),
                ..EnginePolicy::default()
            },
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        });
        let (actions, action_task) = ActionService::spawn(
            ActionDeps {
                actions: store.clone(),
                incidents: store.clone(),
                audit: store.clone(),
                uow: store,
                link,
                redactor: Arc::new(PatternRedactor::default()),
                clock,
                manager: manager.clone(),
                bus,
                shutdown: shutdown.clone(),
            },
            ActionSettings {
                offline_wait: jiff::SignedDuration::ZERO,
                ..ActionSettings::default()
            },
        );
        Self {
            manager,
            actions,
            shutdown,
            manager_task: Some(manager_task),
            action_task: Some(action_task),
        }
    }

    async fn stop(mut self) {
        self.shutdown.cancel();
        join(self.manager_task.take()).await;
        join(self.action_task.take()).await;
    }
}

/// Waits for a service task without sleeping.
async fn join(task: Option<JoinHandle<()>>) {
    let Some(task) = task else {
        return;
    };
    for _ in 0..10_000 {
        if task.is_finished() {
            let _ = task.await;
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("services did not stop");
}
