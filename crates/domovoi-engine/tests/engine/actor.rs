//! The manager task against the in-memory store and the fake clock.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateRef, CommitError, CommitReceipt, Expectation, Expected, IncidentFilter, IncidentRepo,
    IncidentTimelineEntry, JournalRead, Observed, Page, Redactor, SignalRepo, SignalSink,
    StateWrite, StoreError, Transaction, UnitOfWork, Versioned, VersionedRead,
};
use domovoi_core::{
    CheckKey, HostId, Incident, IncidentEvent, IncidentId, IncidentPolicy, IncidentStatus, Note,
    NoteId, Severity, Signal, SignalClass, SourceKind, TriageOutcome, TriageReport, UserId,
    Verdict, apply,
};
use domovoi_engine::redact::REDACTED;
use domovoi_engine::{
    CommandError, DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps,
    ManagerHandle, PatternRedactor,
};
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::SignedDuration;
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::TryRecvError;
use tokio_util::sync::CancellationToken;

use crate::harness::Sig;

/// A unit of work and versioned reader that checks, on every commit, that nothing was
/// published yet, records what it applied, and can be told to fail or conflict.
struct Watched {
    inner: Arc<InMemoryStore>,
    bus: Mutex<broadcast::Receiver<DomainEvent>>,
    fail_writes: AtomicBool,
    conflicts_remaining: AtomicU32,
    attempts: AtomicU32,
    early: AtomicBool,
    commits: Mutex<Vec<Transaction>>,
    /// A status change to apply to one incident just before the manager reads it through
    /// `VersionedRead`, so the race between the finder and the versioned read is exact.
    mutate_before_read: Mutex<Option<(IncidentId, IncidentEvent)>>,
}

impl Watched {
    fn new(inner: Arc<InMemoryStore>, bus: broadcast::Sender<DomainEvent>) -> Self {
        Self {
            inner,
            bus: Mutex::new(bus.subscribe()),
            fail_writes: AtomicBool::new(false),
            conflicts_remaining: AtomicU32::new(0),
            attempts: AtomicU32::new(0),
            early: AtomicBool::new(false),
            commits: Mutex::new(Vec::new()),
            mutate_before_read: Mutex::new(None),
        }
    }

    /// Applies `event` to `id` in the store immediately before the manager's next
    /// `VersionedRead::incident` for it, reproducing a status change that lands between the
    /// finder lookup and the versioned read.
    fn change_before_read(&self, id: IncidentId, event: IncidentEvent) {
        *self.mutate_before_read.lock().unwrap() = Some((id, event));
    }

    fn check_published(&self) {
        if !matches!(
            self.bus.lock().unwrap().try_recv(),
            Err(TryRecvError::Empty)
        ) {
            self.early.store(true, Ordering::SeqCst);
        }
    }

    fn commits(&self) -> Vec<Transaction> {
        self.commits.lock().unwrap().clone()
    }

    fn attempts(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl UnitOfWork for Watched {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        self.check_published();
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(CommitError::Backend("disk full".to_owned()));
        }
        if self.conflicts_remaining.load(Ordering::SeqCst) > 0 {
            self.conflicts_remaining.fetch_sub(1, Ordering::SeqCst);
            return Err(CommitError::Conflict {
                aggregate: AggregateRef::incident(IncidentId::new()),
                expected: Expectation::Version(1),
                found: Observed::Version(2),
            });
        }
        self.commits.lock().unwrap().push(tx.clone());
        self.inner.commit(tx).await
    }
}

#[async_trait]
impl VersionedRead for Watched {
    async fn incident(&self, id: IncidentId) -> Result<Option<Versioned<Incident>>, StoreError> {
        let pending = self.mutate_before_read.lock().unwrap().take();
        if let Some((target, event)) = pending {
            if target == id {
                if let Some(mut incident) = IncidentRepo::get(&*self.inner, id).await? {
                    apply(&mut incident, event, &IncidentPolicy::default(), T0)
                        .map_err(|err| StoreError::Backend(err.to_string()))?;
                    IncidentRepo::update(&*self.inner, &incident).await?;
                }
            } else {
                *self.mutate_before_read.lock().unwrap() = Some((target, event));
            }
        }
        self.inner.incident(id).await
    }

    async fn active_incidents(&self) -> Result<Vec<Versioned<Incident>>, StoreError> {
        self.inner.active_incidents().await
    }

    async fn note(&self, id: NoteId) -> Result<Option<Versioned<Note>>, StoreError> {
        self.inner.note(id).await
    }
}

struct Running {
    handle: ManagerHandle,
    events: broadcast::Receiver<DomainEvent>,
    store: Arc<InMemoryStore>,
    watched: Arc<Watched>,
    clock: Arc<FakeClock>,
    shutdown: CancellationToken,
}

fn start(policy: EnginePolicy) -> Running {
    start_with(policy, Arc::new(PatternRedactor::default()))
}

fn start_with(policy: EnginePolicy, redactor: Arc<dyn Redactor>) -> Running {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let (bus, events) = broadcast::channel(256);
    let watched = Arc::new(Watched::new(store.clone(), bus.clone()));
    let shutdown = CancellationToken::new();
    let deps = ManagerDeps {
        incidents: store.clone(),
        uow: watched.clone(),
        versions: watched.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor,
        policy,
        bus,
        shutdown: shutdown.clone(),
    };
    let (handle, _task) = IncidentManager::spawn(deps);
    Running {
        handle,
        events,
        store,
        watched,
        clock,
        shutdown,
    }
}

fn drain(events: &mut broadcast::Receiver<DomainEvent>) -> Vec<EventKind> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        out.push(event.kind);
    }
    out
}

async fn active(store: &InMemoryStore) -> Vec<Incident> {
    let filter = IncidentFilter {
        active: Some(true),
        ..IncidentFilter::default()
    };
    IncidentRepo::list(store, &filter, &Page::first(100))
        .await
        .unwrap()
        .items
}

async fn stored_version(store: &InMemoryStore, id: IncidentId) -> u64 {
    VersionedRead::incident(store, id)
        .await
        .unwrap()
        .expect("the incident exists")
        .version
}

async fn timeline(store: &InMemoryStore, id: IncidentId) -> Vec<IncidentTimelineEntry> {
    IncidentRepo::timeline(store, id).await.unwrap()
}

fn immediate(host: HostId, check: &'static str, target: Option<&'static str>) -> Sig {
    Sig::new(host, check, target).immediate()
}

#[tokio::test]
async fn events_are_published_only_after_writes() {
    let mut run = start(EnginePolicy::default());
    let power = Sig::new(HostId::new(), "power.supply", None).immediate();
    run.handle.ingest(vec![power.firing(0)]).await.unwrap();
    assert!(
        !run.watched.early.load(Ordering::SeqCst),
        "an event was published before its write"
    );
    assert_eq!(
        drain(&mut run.events),
        [EventKind::IncidentOpened, EventKind::TriageRequested]
    );
    let stored = active(&run.store).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].status(), IncidentStatus::Open);
    assert!(!run.store.timeline(stored[0].id()).await.unwrap().is_empty());
    run.shutdown.cancel();
}

#[tokio::test]
async fn store_error_publishes_nothing() {
    let mut run = start(EnginePolicy::default());
    let power = Sig::new(HostId::new(), "power.supply", None).immediate();
    run.watched.fail_writes.store(true, Ordering::SeqCst);
    let failed = run.handle.ingest(vec![power.firing(0)]).await;
    assert!(matches!(failed, Err(CommandError::Store(_))), "{failed:?}");
    assert!(drain(&mut run.events).is_empty(), "nothing published");
    assert!(active(&run.store).await.is_empty());

    // The store recovers; the same signal now opens the incident once.
    run.watched.fail_writes.store(false, Ordering::SeqCst);
    run.handle.ingest(vec![power.firing(1)]).await.unwrap();
    assert_eq!(
        drain(&mut run.events),
        [EventKind::IncidentOpened, EventKind::TriageRequested]
    );
    assert_eq!(active(&run.store).await.len(), 1);
    run.shutdown.cancel();
}

#[tokio::test]
async fn commands_are_processed_in_order() {
    let mut run = start(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    // Queued back to back without waiting: the manager handles them in send order.
    let (a, b, c) = tokio::join!(
        run.handle.ingest(vec![disk.firing(0)]),
        run.handle.ingest(vec![disk.resolved(1)]),
        run.handle.ingest(vec![disk.firing(2)]),
    );
    a.unwrap();
    b.unwrap();
    c.unwrap();
    assert_eq!(
        drain(&mut run.events),
        [
            EventKind::IncidentOpened,
            EventKind::TriageRequested,
            EventKind::IncidentResolved,
            EventKind::IncidentReopened,
            EventKind::TriageRequested,
        ]
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn signal_sink_ingest_round_trip() {
    let mut run = start(EnginePolicy::default());
    let sink: &dyn SignalSink = &run.handle;
    let host = HostId::new();
    let disk = Sig::new(host, "disk.free", Some("/"));
    sink.submit(vec![disk.firing(0), disk.firing(60)])
        .await
        .unwrap();
    assert_eq!(
        drain(&mut run.events),
        [EventKind::IncidentOpened, EventKind::TriageRequested]
    );
    let stored = active(&run.store).await;
    assert_eq!(stored.len(), 1);
    let signals = SignalRepo::list_for_incident(&*run.store, stored[0].id(), 10)
        .await
        .unwrap();
    assert_eq!(signals.len(), 2, "both signals are stored and linked");

    // A pending incident that clears is dropped and counted.
    let net = Sig::new(host, "net.link", Some("eth0"));
    sink.submit(vec![net.firing(0), net.resolved(10)])
        .await
        .unwrap();
    assert_eq!(run.handle.flaps(), 1);
    assert_eq!(active(&run.store).await.len(), 1);
    run.shutdown.cancel();
    // Once stopped, the sink says so instead of hanging.
    tokio::task::yield_now().await;
}

#[tokio::test]
async fn ticker_drives_ttl_with_fake_clock() {
    let mut policy = EnginePolicy::default();
    policy.incident.ttl = Some(SignedDuration::from_mins(2));
    let mut run = start(policy);
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    drain(&mut run.events);
    let resolved = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            run.clock.advance(SignedDuration::from_secs(30));
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            if drain(&mut run.events).contains(&EventKind::IncidentResolved) {
                return;
            }
        }
    })
    .await;
    assert!(resolved.is_ok(), "the ticker never resolved the incident");
    assert!(active(&run.store).await.is_empty());
    run.shutdown.cancel();
}

#[tokio::test]
async fn dismiss_closes_the_incident_in_the_store() {
    let mut run = start(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    drain(&mut run.events);
    let id = active(&run.store).await[0].id();

    run.handle
        .dismiss(
            id,
            UserId::new(),
            "the backup fills the disk and clears it".to_owned(),
        )
        .await
        .unwrap();
    assert_eq!(drain(&mut run.events), [EventKind::IncidentResolved]);
    assert!(
        active(&run.store).await.is_empty(),
        "the incident is closed"
    );
    let stored = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    assert_eq!(stored.verdict(), Some(Verdict::Expected));
    let timeline = run.store.timeline(id).await.unwrap();
    assert!(
        timeline.iter().any(|e| e.kind == "human.dismissed"),
        "the reason is in the timeline"
    );

    // The cache does not know the incident, so dismissing it again says there is none.
    let again = run
        .handle
        .dismiss(id, UserId::new(), "again".to_owned())
        .await
        .unwrap_err();
    assert!(
        matches!(again, CommandError::UnknownIncident(_)),
        "{again:?}"
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn one_command_is_one_commit() {
    let run = start(EnginePolicy::default());
    let power = immediate(HostId::new(), "power.supply", None);
    run.handle.ingest(vec![power.firing(0)]).await.unwrap();

    let commits = run.watched.commits();
    assert_eq!(commits.len(), 1, "one commit per command");
    let writes = &commits[0].writes;
    assert!(
        matches!(
            &writes[0],
            StateWrite::Incident {
                expected: Expected::New,
                ..
            }
        ),
        "{writes:?}"
    );
    assert!(
        matches!(&writes[1], StateWrite::Signal { .. }),
        "{writes:?}"
    );
    assert!(
        writes[2..]
            .iter()
            .all(|write| matches!(write, StateWrite::IncidentEvent(_))),
        "{writes:?}"
    );
    assert!(
        !writes[2..].is_empty(),
        "an opening incident has a timeline"
    );

    let id = active(&run.store).await[0].id();
    run.handle.ack(id, UserId::new()).await.unwrap();

    let commits = run.watched.commits();
    assert_eq!(commits.len(), 2, "the ack is one commit too");
    let writes = &commits[1].writes;
    assert!(
        matches!(
            &writes[0],
            StateWrite::Incident {
                expected: Expected::Version(1),
                ..
            }
        ),
        "{writes:?}"
    );
    assert!(
        writes[1..]
            .iter()
            .all(|write| matches!(write, StateWrite::IncidentEvent(_))),
        "{writes:?}"
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn timeline_entries_are_written_to_incident_events_and_mirrored_into_the_journal() {
    let run = start(EnginePolicy::default());
    let power = immediate(HostId::new(), "power.supply", None);
    run.handle.ingest(vec![power.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();

    let timeline = timeline(&run.store, id).await;
    assert!(!timeline.is_empty());
    let journal = JournalRead::entries_for(&*run.store, &AggregateRef::incident(id), 500)
        .await
        .unwrap();
    let mirrored: Vec<_> = journal
        .iter()
        .filter(|entry| entry.kind == "incident.timeline")
        .collect();
    assert_eq!(
        mirrored.len(),
        timeline.len(),
        "every timeline entry has one mirrored journal row"
    );
    for (entry, row) in timeline.iter().zip(&mirrored) {
        assert_eq!(row.payload["kind"], entry.kind);
        assert_eq!(row.payload["summary"], entry.summary);
        assert_eq!(row.payload["detail"], entry.detail);
        assert_eq!(row.occurred_at, entry.at);
    }
    run.shutdown.cancel();
}

#[tokio::test]
async fn versions_follow_every_write() {
    let run = start(EnginePolicy::default());
    let disk = immediate(HostId::new(), "disk.free", Some("/"));
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();
    assert_eq!(stored_version(&run.store, id).await, 1);

    run.handle.ack(id, UserId::new()).await.unwrap();
    assert_eq!(stored_version(&run.store, id).await, 2);

    let until = T0.checked_add(SignedDuration::from_hours(1)).unwrap();
    run.handle.mute(id, until, UserId::new()).await.unwrap();
    assert_eq!(stored_version(&run.store, id).await, 3);
    assert_eq!(
        run.handle.write_conflicts(),
        0,
        "every command saw the version it had written"
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn conflict_is_retried_once_after_reload() {
    let run = start(EnginePolicy::default());
    run.watched.conflicts_remaining.store(1, Ordering::SeqCst);
    let power = immediate(HostId::new(), "power.supply", None);
    run.handle.ingest(vec![power.firing(0)]).await.unwrap();

    assert_eq!(run.watched.attempts(), 2, "one conflict, then one applied");
    assert_eq!(
        run.watched.commits().len(),
        1,
        "only the applied commit ran"
    );
    assert_eq!(run.handle.write_conflicts(), 1);

    let id = active(&run.store).await[0].id();
    let entries = timeline(&run.store, id).await;
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.kind == "incident.created")
            .count(),
        1,
        "the applied commit wrote the timeline once"
    );
    let signals = SignalRepo::list_for_incident(&*run.store, id, 10)
        .await
        .unwrap();
    assert_eq!(signals.len(), 1, "the signal was stored once");
    run.shutdown.cancel();
}

#[tokio::test]
async fn second_conflict_fails_the_command_and_publishes_nothing() {
    let mut run = start(EnginePolicy::default());
    run.watched.conflicts_remaining.store(2, Ordering::SeqCst);
    let power = immediate(HostId::new(), "power.supply", None);
    let failed = run.handle.ingest(vec![power.firing(0)]).await;
    assert!(matches!(failed, Err(CommandError::Store(_))), "{failed:?}");
    assert_eq!(run.watched.attempts(), 2, "the retry ran and failed too");
    assert!(drain(&mut run.events).is_empty(), "nothing published");
    assert!(active(&run.store).await.is_empty());

    // The next command sees a clean store and succeeds.
    run.handle.ingest(vec![power.firing(1)]).await.unwrap();
    assert_eq!(active(&run.store).await.len(), 1);
    run.shutdown.cancel();
}

#[tokio::test]
async fn outside_writer_makes_the_cache_stale_and_the_manager_recovers() {
    let mut run = start(EnginePolicy::default());
    let disk = immediate(HostId::new(), "disk.free", Some("/"));
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();
    drain(&mut run.events);

    let mut outside = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    apply(
        &mut outside,
        domovoi_core::IncidentEvent::Firing {
            severity: Severity::Critical,
            class: SignalClass::Immediate,
            observed_at: T0,
            value: None,
        },
        &IncidentPolicy::default(),
        T0,
    )
    .unwrap();
    IncidentRepo::update(&*run.store, &outside).await.unwrap();
    assert_eq!(stored_version(&run.store, id).await, 2);

    run.handle.ack(id, UserId::new()).await.unwrap();
    assert_eq!(run.handle.write_conflicts(), 1, "the cache was stale");

    let stored = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    assert_eq!(
        stored.severity(),
        Severity::Critical,
        "the outside writer's severity survived"
    );
    assert_eq!(
        stored.status(),
        IncidentStatus::Acked,
        "the command was applied on top"
    );
    assert_eq!(stored_version(&run.store, id).await, 3);
    run.shutdown.cancel();
}

#[tokio::test]
async fn redacted_secret_never_reaches_incident_timeline_or_journal() {
    let secret = "hunter2-super-secret";
    let redactor = Arc::new(PatternRedactor::new([SecretString::from(
        secret.to_owned(),
    )]));
    let run = start_with(EnginePolicy::default(), redactor);

    let signal = Signal::firing(
        SourceKind::Agent,
        HostId::new(),
        CheckKey::parse("disk.free").unwrap(),
        None,
        Severity::Warning,
        format!("disk free low for {secret}"),
        T0,
    )
    .with_class(SignalClass::Immediate);
    run.handle.ingest(vec![signal]).await.unwrap();
    let id = active(&run.store).await[0].id();

    let stored = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    assert!(!stored.title().contains(secret), "{}", stored.title());
    assert!(stored.title().contains(REDACTED));

    let signals = SignalRepo::list_for_incident(&*run.store, id, 10)
        .await
        .unwrap();
    assert!(!signals.is_empty());
    for signal in &signals {
        assert!(!signal.summary().contains(secret), "{}", signal.summary());
        assert!(signal.summary().contains(REDACTED));
    }

    let entries = timeline(&run.store, id).await;
    assert!(!entries.is_empty());
    for entry in &entries {
        assert!(!entry.summary.contains(secret), "{}", entry.summary);
    }
    assert!(entries.iter().any(|entry| entry.summary.contains(REDACTED)));

    let journal = JournalRead::entries_for(&*run.store, &AggregateRef::incident(id), 500)
        .await
        .unwrap();
    assert!(!journal.is_empty());
    for row in &journal {
        assert!(!row.payload.to_string().contains(secret), "{}", row.payload);
    }
    run.shutdown.cancel();
}

#[tokio::test]
async fn reopening_a_resolved_incident_updates_it_with_the_loaded_version() {
    let run = start(EnginePolicy::default());
    let disk = immediate(HostId::new(), "disk.free", Some("/"));
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();
    run.handle.ingest(vec![disk.resolved(10)]).await.unwrap();
    assert!(active(&run.store).await.is_empty());
    let resolved_version = stored_version(&run.store, id).await;

    run.handle.ingest(vec![disk.firing(20)]).await.unwrap();
    assert_eq!(run.handle.write_conflicts(), 0, "the loaded version fitted");
    assert_eq!(active(&run.store).await.len(), 1);
    let stored = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    assert_eq!(stored.status(), IncidentStatus::Open);
    assert!(
        stored_version(&run.store, id).await > resolved_version,
        "the reopen advanced the stored version"
    );
    run.shutdown.cancel();
}

fn report() -> TriageReport {
    TriageReport {
        headline: "The disk is filling".to_owned(),
        summary: "A backup writes more than the disk has left.".to_owned(),
        cause: "the nightly backup".to_owned(),
        evidence: Vec::new(),
        verdict: Verdict::ActionNeeded,
        next_step: "free space".to_owned(),
        manual_steps: Vec::new(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

#[tokio::test]
async fn attaching_triage_to_a_resolved_incident_uses_its_stored_version() {
    let run = start(EnginePolicy::default());
    let disk = immediate(HostId::new(), "disk.free", Some("/"));
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();
    run.handle.ingest(vec![disk.resolved(10)]).await.unwrap();
    assert!(active(&run.store).await.is_empty());
    let resolved_version = stored_version(&run.store, id).await;

    run.handle
        .attach_triage(
            id,
            TriageOutcome::Report(report()),
            std::collections::BTreeMap::new(),
        )
        .await
        .unwrap();
    assert_eq!(run.handle.write_conflicts(), 0, "the loaded version fitted");
    assert!(
        stored_version(&run.store, id).await > resolved_version,
        "attaching triage advanced the stored version"
    );
    let stored = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    assert_eq!(stored.verdict(), Some(Verdict::ActionNeeded));
    run.shutdown.cancel();
}

#[tokio::test]
async fn discarding_a_pending_incident_is_one_commit() {
    let run = start(EnginePolicy::default());
    let host = HostId::new();
    let net = Sig::new(host, "net.link", Some("eth0"));
    run.handle.ingest(vec![net.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();
    let earlier = timeline(&run.store, id).await;
    assert!(!earlier.is_empty());

    run.handle.ingest(vec![net.resolved(60)]).await.unwrap();
    assert!(active(&run.store).await.is_empty(), "the incident is gone");
    assert_eq!(run.handle.flaps(), 1);

    let commits = run.watched.commits();
    let last = commits.last().expect("the resolve command committed");
    let discard = last
        .writes
        .iter()
        .find(|write| matches!(write, StateWrite::DiscardPendingIncident { .. }))
        .expect("the resolve command discards the pending incident");
    assert!(
        matches!(
            discard,
            StateWrite::DiscardPendingIncident { expected: 1, .. }
        ),
        "{discard:?}"
    );
    let journal = JournalRead::entries_for(&*run.store, &AggregateRef::incident(id), 500)
        .await
        .unwrap();
    assert!(
        !journal.is_empty(),
        "journal rows from the opening commit are kept"
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn an_active_incident_that_resolves_before_the_versioned_read_is_reopened() {
    let mut run = start(EnginePolicy::default());
    // Let the manager load its cache, so the incident below is a genuine cache miss.
    let warm_up = immediate(HostId::new(), "cpu.load", None);
    run.handle.ingest(vec![warm_up.firing(0)]).await.unwrap();
    drain(&mut run.events);

    let host = HostId::new();
    let disk = immediate(host, "disk.free", Some("/"));
    let signal = disk.firing(0);
    // An active incident the manager has not cached, as if another writer opened it.
    let incident = open_incident(host, "disk.free", Some("/")).unwrap();
    let id = incident.id();
    IncidentRepo::insert(&*run.store, &incident).await.unwrap();
    assert_eq!(stored_version(&run.store, id).await, 1);

    // The active finder sees it; it resolves before the versioned read.
    run.watched
        .change_before_read(id, IncidentEvent::Resolved { observed_at: T0 });
    run.handle.ingest(vec![signal.clone()]).await.unwrap();

    let stored = IncidentRepo::get(&*run.store, id).await.unwrap().unwrap();
    assert_eq!(
        stored.status(),
        IncidentStatus::Open,
        "the resolved incident was reopened"
    );
    let active_for_fingerprint =
        IncidentRepo::find_active_by_fingerprint(&*run.store, &signal.fingerprint())
            .await
            .unwrap();
    assert_eq!(
        active_for_fingerprint.map(|incident| incident.id()),
        Some(id),
        "the original incident was reused, not duplicated"
    );
    assert_eq!(
        stored_version(&run.store, id).await,
        3,
        "insert, the outside resolution, then the reopen"
    );
    let signals = SignalRepo::list_for_incident(&*run.store, id, 10)
        .await
        .unwrap();
    assert_eq!(
        signals.len(),
        1,
        "the signal is linked to the reopened incident"
    );
    run.shutdown.cancel();
}

#[tokio::test]
async fn a_resolved_incident_that_reopens_before_the_versioned_read_is_folded_into() {
    let run = start(EnginePolicy::default());
    let host = HostId::new();
    let disk = immediate(host, "disk.free", Some("/"));
    run.handle.ingest(vec![disk.firing(0)]).await.unwrap();
    let id = active(&run.store).await[0].id();
    run.handle.ingest(vec![disk.resolved(10)]).await.unwrap();
    assert!(active(&run.store).await.is_empty());
    let resolved_version = stored_version(&run.store, id).await;

    // The resolved finder sees it; it reopens before the versioned read.
    run.watched.change_before_read(
        id,
        IncidentEvent::Firing {
            severity: Severity::Warning,
            class: SignalClass::Immediate,
            observed_at: T0,
            value: None,
        },
    );
    run.handle.ingest(vec![disk.firing(20)]).await.unwrap();

    let active = active(&run.store).await;
    assert_eq!(active.len(), 1, "no duplicate incident was created");
    assert_eq!(active[0].id(), id, "the existing incident was used");
    assert_eq!(
        stored_version(&run.store, id).await,
        resolved_version + 2,
        "the outside reopen, then the manager folding the signal in"
    );
    let signals = SignalRepo::list_for_incident(&*run.store, id, 10)
        .await
        .unwrap();
    assert_eq!(
        signals.len(),
        3,
        "the new signal is linked to the existing incident"
    );
    run.shutdown.cancel();
}
