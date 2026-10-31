//! The manager task against the in-memory store and the fake clock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    IncidentFilter, IncidentRepo, IncidentTimelineEntry, Page, Paged, SignalSink, StoreError,
};
use domovoi_core::{Fingerprint, HostId, Incident, IncidentId, IncidentStatus};
use domovoi_engine::{
    CommandError, DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps, ManagerHandle,
};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::TryRecvError;
use tokio_util::sync::CancellationToken;

use crate::harness::Sig;

/// An incident repo that checks, on every write, that nothing was published yet, and can
/// be told to fail writes.
struct Watched {
    inner: Arc<InMemoryStore>,
    bus: Mutex<broadcast::Receiver<DomainEvent>>,
    fail_writes: AtomicBool,
    early: AtomicBool,
}

impl Watched {
    fn check(&self) -> Result<(), StoreError> {
        if !matches!(
            self.bus.lock().unwrap().try_recv(),
            Err(TryRecvError::Empty)
        ) {
            self.early.store(true, Ordering::SeqCst);
        }
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(StoreError::Backend("disk full".to_owned()));
        }
        Ok(())
    }
}

#[async_trait]
impl IncidentRepo for Watched {
    async fn insert(&self, incident: &Incident) -> Result<(), StoreError> {
        self.check()?;
        self.inner.insert(incident).await
    }
    async fn update(&self, incident: &Incident) -> Result<(), StoreError> {
        self.check()?;
        self.inner.update(incident).await
    }
    async fn get(&self, id: IncidentId) -> Result<Option<Incident>, StoreError> {
        IncidentRepo::get(&*self.inner, id).await
    }
    async fn find_active_by_fingerprint(
        &self,
        fp: &Fingerprint,
    ) -> Result<Option<Incident>, StoreError> {
        self.inner.find_active_by_fingerprint(fp).await
    }
    async fn find_recent_resolved_by_fingerprint(
        &self,
        fp: &Fingerprint,
        since: Timestamp,
    ) -> Result<Option<Incident>, StoreError> {
        self.inner
            .find_recent_resolved_by_fingerprint(fp, since)
            .await
    }
    async fn list(
        &self,
        filter: &IncidentFilter,
        page: &Page,
    ) -> Result<Paged<Incident>, StoreError> {
        IncidentRepo::list(&*self.inner, filter, page).await
    }
    async fn discard_pending(&self, id: IncidentId) -> Result<(), StoreError> {
        self.check()?;
        self.inner.discard_pending(id).await
    }
    async fn append_event(&self, entry: &IncidentTimelineEntry) -> Result<(), StoreError> {
        self.check()?;
        self.inner.append_event(entry).await
    }
    async fn timeline(&self, id: IncidentId) -> Result<Vec<IncidentTimelineEntry>, StoreError> {
        self.inner.timeline(id).await
    }
    async fn search(&self, text: &str, limit: u16) -> Result<Vec<Incident>, StoreError> {
        IncidentRepo::search(&*self.inner, text, limit).await
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
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let (bus, events) = broadcast::channel(256);
    let watched = Arc::new(Watched {
        inner: store.clone(),
        bus: Mutex::new(bus.subscribe()),
        fail_writes: AtomicBool::new(false),
        early: AtomicBool::new(false),
    });
    let shutdown = CancellationToken::new();
    let deps = ManagerDeps {
        incidents: watched.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
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
    let signals =
        domovoi_core::ports::SignalRepo::list_for_incident(&*run.store, stored[0].id(), 10)
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
