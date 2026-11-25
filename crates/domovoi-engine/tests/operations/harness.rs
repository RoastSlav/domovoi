//! A running operation service over fakes: one in-memory store, a fake clock, one host with
//! an enrolled agent that offers `restart_container`, and the helpers a writer test needs.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::drive::Gated;
use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateRef, AuditEvent, AuditFilter, AuditLog, CommitError, CommitReceipt, EffectRead,
    EffectRecord, EffectStatus, Expectation, HostRepo, JournalEntry, JournalRead, Observed, Page,
    Presence, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{
    Actor, AgentIdentity, CheckKey, ContractId, ExecutionIntent, HostId, JournalEpoch, OperationId,
    OperationKind, OperationSnapshot, ParamKind, ParamName, ParamSpec, PinnedSubject, PlanBinding,
    Risk, SkillSummary, StepId, TargetKey, UserId,
};
use domovoi_engine::Evidence;
use domovoi_engine::operations::{
    ExecutionRequest, OperationDeps, OperationHandle, OperationSeed, OperationService,
    OperationSettings,
};
use domovoi_testkit::build::{T0, manifest};
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakeTargetResolver, InMemoryStore,
    ListRedactor, approvals,
};
use jiff::SignedDuration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The host every test operation acts on.
pub fn host_id() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

/// The skill the fake agent offers: restarts a named container.
pub fn restart_skill() -> SkillSummary {
    let mut manifest = manifest("restart_container", Risk::Write).unwrap();
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").unwrap(),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

/// A unit of work that makes the next commits that write an operation fail with a conflict.
pub struct Flaky {
    inner: Arc<InMemoryStore>,
    conflicts: AtomicU32,
}

impl Flaky {
    /// A wrapper that conflicts `conflicts` times, then passes commits through.
    pub fn new(inner: Arc<InMemoryStore>, conflicts: u32) -> Arc<Self> {
        Arc::new(Self {
            inner,
            conflicts: AtomicU32::new(conflicts),
        })
    }
}

impl Flaky {
    /// Makes the next `n` operation commits conflict.
    pub fn conflict_next(&self, n: u32) {
        self.conflicts.store(n, Ordering::SeqCst);
    }
}

#[async_trait]
impl UnitOfWork for Flaky {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        let writes_operation = tx
            .writes
            .iter()
            .any(|write| matches!(write, StateWrite::Operation(_)));
        if writes_operation
            && self
                .conflicts
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
        {
            return Err(CommitError::Conflict {
                aggregate: AggregateRef::operation(OperationId::new()),
                expected: Expectation::Version(1),
                found: Observed::Version(2),
            });
        }
        self.inner.commit(tx).await
    }
}

/// The running service and what a test needs to drive and read it.
pub struct World {
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    pub registry: Arc<FakeCapabilityRegistry>,
    pub resolver: Arc<FakeTargetResolver>,
    pub uow: Arc<dyn UnitOfWork>,
    pub settings: OperationSettings,
    pub handle: Option<OperationHandle>,
    pub task: Option<tokio::task::JoinHandle<()>>,
    pub queue: Arc<Gated>,
    pub shutdown: CancellationToken,
    pub user: UserId,
    pub bus: tokio::sync::broadcast::Sender<domovoi_engine::events::DomainEvent>,
}

impl Drop for World {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// A world with the default settings.
pub async fn world() -> World {
    world_with(OperationSettings::default()).await
}

/// A world with `settings`.
pub async fn world_with(settings: OperationSettings) -> World {
    let store = Arc::new(InMemoryStore::new());
    world_over(settings, store.clone(), store).await
}

/// A world whose runner claims nothing until `world.queue.open()`, so a test can look at,
/// or claim by hand, what a commit queued.
pub async fn world_gated() -> World {
    let store = Arc::new(InMemoryStore::new());
    build(OperationSettings::default(), store.clone(), store, true).await
}

/// A world whose commits go through `uow`, which must forward to `store`.
pub async fn world_over(
    settings: OperationSettings,
    store: Arc<InMemoryStore>,
    uow: Arc<dyn UnitOfWork>,
) -> World {
    build(settings, store, uow, false).await
}

async fn build(
    settings: OperationSettings,
    store: Arc<InMemoryStore>,
    uow: Arc<dyn UnitOfWork>,
    closed: bool,
) -> World {
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    link.set_skills(host_id(), vec![restart_skill()]);
    // Every dispatch records the journal it was sent under, so the agent has one.
    link.set_journal_epoch(host_id(), JournalEpoch::from_uuid(Uuid::now_v7()));
    link.on_skill(|_, _| Ok(crate::drive::ok_output("restarted plex")));
    link.on_check(|_, _, _| Ok(crate::drive::resolved()));
    let mut registered = domovoi_testkit::build::host("nas").unwrap();
    registered.id = host_id();
    HostRepo::upsert_host(&*store, &registered).await.unwrap();
    HostRepo::put_agent(
        &*store,
        &AgentIdentity {
            agent: link.agent_id(host_id()),
            host: host_id(),
            public_key: [7; 32],
            version: "0.1.0".to_owned(),
            enrolled_at: T0,
            revoked_at: None,
        },
    )
    .await
    .unwrap();
    let registry = Arc::new(FakeCapabilityRegistry::following(link.clone()));
    let resolver = Arc::new(FakeTargetResolver::new());
    let shutdown = CancellationToken::new();
    let store_for_queue = store.clone();
    let mut world = World {
        store,
        uow,
        clock,
        link,
        registry,
        resolver,
        settings,
        handle: None,
        task: None,
        queue: Gated::new(store_for_queue, closed),
        shutdown,
        user: UserId::new(),
        bus: tokio::sync::broadcast::channel(64).0,
    };
    let (handle, task) = world.start();
    world.handle = Some(handle);
    world.task = Some(task);
    world
}

/// The operation's one pinned subject: the `plex` container check on the test host.
pub fn subject() -> PinnedSubject {
    PinnedSubject {
        host: host_id(),
        check: CheckKey::parse("container.down").unwrap(),
        target: Some(TargetKey::parse("plex").unwrap()),
    }
}

/// A seed for an operation with no first steps.
pub fn seed() -> OperationSeed {
    OperationSeed {
        kind: OperationKind::Investigate,
        goal: "find out why plex is down".to_owned(),
        pinned: vec![subject()],
        incidents: Vec::new(),
        parent: None,
        budgets: None,
        first_steps: Vec::new(),
        effects: Vec::new(),
        actor: Actor::System("tests".to_owned()),
    }
}

/// A request to restart the container `name` on the test host.
pub fn restart(name: &str) -> ExecutionRequest {
    let mut args = BTreeMap::new();
    args.insert("container".to_owned(), name.to_owned());
    ExecutionRequest {
        intent: ExecutionIntent {
            contract: ContractId::parse("skill.restart_container").unwrap(),
            contract_version: 1,
            host: host_id(),
            check: CheckKey::parse("container.down").unwrap(),
            target: Some(TargetKey::parse("plex").unwrap()),
            args,
            retry_of: None,
        },
        parent: None,
        caused_by: None,
    }
}

impl World {
    /// Starts a service over this world's store, as a hub start does.
    pub fn start(&self) -> (OperationHandle, tokio::task::JoinHandle<()>) {
        let caps = action_caps(self.link.clone());
        OperationService::spawn(
            OperationDeps {
                bus: self.bus.clone(),
                operations: self.store.clone(),
                experience: self.store.clone(),
                uow: self.uow.clone(),
                effects: self.queue.clone(),
                effect_read: self.store.clone(),
                advisories: self.store.clone(),
                incidents: self.store.clone(),
                hosts: self.store.clone(),
                evidence: Evidence::new(
                    evidence_caps(self.link.clone()),
                    self.clock.clone(),
                    self.registry.clone(),
                ),
                registry: self.registry.clone(),
                resolver: self.resolver.clone(),
                observer: observer_of(self.link.clone()),
                journal: caps.journal,
                handoff: caps.handoff,
                rerun: caps.rerun,
                redactor: Arc::new(ListRedactor::new(["hunter2"])),
                clock: self.clock.clone(),
                shutdown: self.shutdown.clone(),
                investigate: None,
                autonomy: None,
            },
            self.settings.clone(),
        )
    }

    /// The running service.
    pub fn handle(&self) -> &OperationHandle {
        self.handle.as_ref().expect("the world has a service")
    }

    /// Creates an operation from the default seed.
    pub async fn create(&self) -> OperationId {
        self.handle().create(seed()).await.unwrap()
    }

    /// The operation as stored.
    pub async fn load(&self, id: OperationId) -> OperationSnapshot {
        use domovoi_core::ports::OperationRepo;
        OperationRepo::load(&*self.store, id)
            .await
            .unwrap()
            .unwrap()
    }

    /// The outcome of `step` as stored.
    pub async fn outcome(&self, op: OperationId, step: StepId) -> domovoi_core::StepOutcome {
        self.load(op).await.step(step).unwrap().progress.outcome
    }

    /// Proposes the restart of `plex` and returns the Execution step.
    pub async fn propose(&self, op: OperationId) -> StepId {
        self.handle()
            .propose_execution(op, restart("plex"))
            .await
            .unwrap()
    }

    /// The plan binding a person would see for Execution step `step`.
    pub async fn binding(&self, op: OperationId, step: StepId) -> PlanBinding {
        PlanBinding::Full(
            self.load(op)
                .await
                .execution_of(step)
                .unwrap()
                .value
                .plan_hash,
        )
    }

    /// Approves `step` as the world's user, bound to the stored plan.
    pub async fn approve(
        &self,
        op: OperationId,
        step: StepId,
    ) -> Result<(), domovoi_engine::operations::OperationError> {
        let binding = self.binding(op, step).await;
        self.handle()
            .authorize(
                op,
                step,
                approvals::authorize(approvals::human(self.user)),
                binding,
                approvals::provenance(),
                None,
            )
            .await
    }

    /// Every effect in `status` that belongs to `op`.
    pub async fn effects(&self, op: OperationId, status: EffectStatus) -> Vec<EffectRecord> {
        EffectRead::effects_with_status(&*self.store, status, 500)
            .await
            .unwrap()
            .into_iter()
            .filter(|effect| effect.aggregate == AggregateRef::operation(op))
            .collect()
    }

    /// Every journal entry of `op`.
    pub async fn journal(&self, op: OperationId) -> Vec<JournalEntry> {
        JournalRead::entries_for(&*self.store, &AggregateRef::operation(op), 500)
            .await
            .unwrap()
    }

    /// The kinds of every audit event, oldest first.
    pub async fn audit_kinds(&self) -> Vec<String> {
        let page = AuditLog::list(&*self.store, &AuditFilter::default(), &Page::first(500))
            .await
            .unwrap();
        page.items
            .into_iter()
            .rev()
            .map(|event| event.kind)
            .collect()
    }

    /// The newest audit event of `kind`.
    pub async fn audit_event(&self, kind: &str) -> Option<AuditEvent> {
        let filter = AuditFilter {
            kind: Some(kind.to_owned()),
            ..AuditFilter::default()
        };
        let page = AuditLog::list(&*self.store, &filter, &Page::first(1))
            .await
            .unwrap();
        page.items.into_iter().next()
    }

    /// The fake clock's time.
    pub fn now(&self) -> jiff::Timestamp {
        domovoi_core::ports::Clock::now(&*self.clock)
    }

    /// Moves the clock on.
    pub fn advance(&self, seconds: i64) {
        self.clock.advance(SignedDuration::from_secs(seconds));
    }
}
