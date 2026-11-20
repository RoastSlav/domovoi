//! A running incident manager and operation service over a crashing store, plus the restart.
//!
//! Every commit the services make goes through [`CrashingStore`]; every read goes straight
//! to the store underneath it, so a restart sees exactly what was durable. The world is
//! built twice per sweep case: once over a fresh store to run the scenario, and once over
//! the surviving store after a simulated crash.

use std::sync::Arc;

use domovoi_core::ports::{
    AdvisoryRepo, AggregateKind, CheckRecheck, Clock, EffectQueue, EffectRead, EffectStatus,
    HostRepo, IncidentFilter, IncidentRepo, OperationRepo, Page, Presence, SkillOutput,
};
use domovoi_core::{
    Actor, AgentId, AgentIdentity, CheckKey, Evidence, HostId, Incident, IncidentId, JournalEpoch,
    OperationKind, ParamKind, ParamName, ParamSpec, PinnedSubject, Risk, Severity, Signal,
    SignalClass, SignalStatus, SkillSummary, SourceKind, TargetKey, VerifySpec,
};
use domovoi_engine::operations::{
    OperationDeps, OperationHandle, OperationSeed, OperationService, OperationSettings,
};
use domovoi_engine::{EnginePolicy, IncidentManager, ManagerDeps, ManagerHandle, PatternRedactor};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::crash::{CrashingStore, SweepStore, check_invariants};
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakeTargetResolver};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// What an operation over the world's container incident is seeded with.
#[must_use]
pub fn operation_seed(incidents: Vec<IncidentId>) -> OperationSeed {
    OperationSeed {
        kind: OperationKind::Investigate,
        goal: "find out why nginx is restarting".to_owned(),
        pinned: vec![PinnedSubject {
            host: host_id(),
            check: CheckKey::parse("docker.container").expect("a valid check"),
            target: Some(TargetKey::parse("nginx").expect("a valid target")),
        }],
        incidents,
        parent: None,
        budgets: None,
        first_steps: Vec::new(),
        effects: Vec::new(),
        actor: Actor::System("crash-sweep".to_owned()),
    }
}

/// The host whose agent offers the skills and whose container incident the world seeds.
#[must_use]
pub fn host_id() -> HostId {
    "01900000-0000-7000-8000-000000000001"
        .parse()
        .expect("a valid host id")
}

/// A second host, used to prove the manager still opens incidents after a restart.
#[must_use]
pub fn new_host_id() -> HostId {
    "01900000-0000-7000-8000-000000000002"
        .parse()
        .expect("a valid host id")
}

/// How long a quiet incident stays open: long enough for a scenario, short enough that a
/// tick after the clock moves resolves the seeded one.
const TTL: SignedDuration = SignedDuration::from_mins(10);

/// The manager policy: two runs confirm a normal signal, and the ticker never fires on its
/// own, so every tick in these tests is one the scenario asked for.
fn policy() -> EnginePolicy {
    let mut policy = EnginePolicy::default();
    policy.incident.ttl = Some(TTL);
    policy.tick_every = SignedDuration::from_hours(24);
    policy
}

/// `restart_container` with one `container` parameter.
#[must_use]
pub fn restart_skill() -> SkillSummary {
    let mut manifest = manifest("restart_container", Risk::Write).expect("a valid manifest");
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").expect("a valid parameter name"),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

/// `restart_container` that re-checks after 10 s, so a successful run is verified.
#[must_use]
pub fn verified_restart() -> SkillSummary {
    let mut skill = restart_skill();
    skill.verify = Some(VerifySpec {
        recheck: true,
        delay: SignedDuration::from_secs(10),
    });
    skill
}

/// A successful `run_skill` output.
#[must_use]
pub fn ok_output(stdout: &str) -> SkillOutput {
    SkillOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        stderr: String::new(),
        truncated: false,
        duration_ms: 1200,
        argv_hash: [0xab; 32],
    }
}

/// A re-check answer for the given status.
#[must_use]
pub fn check(status: SignalStatus) -> CheckRecheck {
    CheckRecheck {
        status,
        summary: "nginx is running".to_owned(),
        evidence: Evidence::new(),
        observed_at: T0,
    }
}

/// A firing signal observed now, from the host's agent.
#[must_use]
pub fn firing(
    host: HostId,
    check: &str,
    target: Option<&str>,
    class: SignalClass,
    at: Timestamp,
) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).expect("a valid check key"),
        target.map(|target| TargetKey::parse(target).expect("a valid target key")),
        Severity::Warning,
        format!("{check} firing"),
        at,
    )
    .with_class(class)
}

/// A resolved signal observed at `at`.
#[must_use]
pub fn resolved(host: HostId, check: &str, target: Option<&str>, at: Timestamp) -> Signal {
    Signal::resolved(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).expect("a valid check key"),
        target.map(|target| TargetKey::parse(target).expect("a valid target key")),
        format!("{check} ok"),
        at,
    )
}

/// A manager, an operation service and their fakes, all over one crashing store.
pub struct World<S: SweepStore> {
    /// The wrapper every commit passes through.
    pub crashing: Arc<CrashingStore<S>>,
    /// The store underneath it: what the services read and what survives a restart.
    pub store: Arc<S>,
    /// Time, moved only by the scenario.
    pub clock: Arc<FakeClock>,
    /// The manager.
    pub manager: ManagerHandle,
    /// The operation service.
    pub operations: OperationHandle,
    shutdown: CancellationToken,
    manager_task: Option<JoinHandle<()>>,
    operation_task: Option<JoinHandle<()>>,
}

impl<S: SweepStore> Drop for World<S> {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl<S: SweepStore + EffectQueue + AdvisoryRepo + HostRepo + OperationRepo> World<S> {
    /// A world over a fresh store, with one open container incident already stored.
    pub async fn new(store: Arc<S>) -> Self {
        let incident = open_incident(host_id(), "docker.container", Some("nginx"))
            .expect("a valid open incident");
        IncidentRepo::insert(&*store, &incident)
            .await
            .expect("the seeded incident is stored");
        Self::build(store).await
    }

    /// A world over the state a crashed process left behind: no seeding, fresh services and
    /// a fresh [`CrashingStore`] that only records.
    pub async fn restarted(store: Arc<S>) -> Self {
        Self::build(store).await
    }

    async fn build(store: Arc<S>) -> Self {
        let mut registered = domovoi_testkit::build::host("crash").unwrap();
        registered.id = host_id();
        HostRepo::upsert_host(&*store, &registered).await.unwrap();
        let agent_id = AgentId::new();
        if HostRepo::live_agent(&*store, host_id())
            .await
            .unwrap()
            .is_none()
        {
            HostRepo::put_agent(
                &*store,
                &AgentIdentity {
                    agent: agent_id,
                    host: host_id(),
                    public_key: [7; 32],
                    version: "0.1.0".to_owned(),
                    enrolled_at: T0,
                    revoked_at: None,
                },
            )
            .await
            .unwrap();
        }
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        link.set_agent(host_id(), agent_id);
        script(&link);
        let crashing = Arc::new(CrashingStore::new(store.clone()));
        let (bus, _events) = broadcast::channel(256);
        let shutdown = CancellationToken::new();
        let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
            incidents: store.clone(),
            uow: crashing.clone(),
            versions: crashing.clone(),
            acks: store.clone(),
            clock: clock.clone(),
            redactor: Arc::new(PatternRedactor::default()),
            policy: policy(),
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        });
        let registry = Arc::new(FakeCapabilityRegistry::following(link.clone()));
        let caps = action_caps(link.clone());
        let (operations, operation_task) = OperationService::spawn(
            OperationDeps {
                bus: tokio::sync::broadcast::channel(16).0,
                operations: store.clone(),
                uow: crashing.clone(),
                effects: store.clone(),
                effect_read: store.clone(),
                advisories: store.clone(),
                incidents: store.clone(),
                hosts: store.clone(),
                evidence: domovoi_engine::Evidence::new(
                    evidence_caps(link.clone()),
                    clock.clone(),
                    registry.clone(),
                ),
                registry,
                resolver: Arc::new(FakeTargetResolver::new()),
                observer: observer_of(link.clone()),
                journal: caps.journal,
                handoff: caps.handoff,
                rerun: caps.rerun,
                redactor: Arc::new(PatternRedactor::default()),
                clock: clock.clone(),
                shutdown: shutdown.clone(),
                investigate: None,
            },
            OperationSettings::default(),
        );
        Self {
            crashing,
            store,
            clock,
            manager,
            operations,
            shutdown,
            manager_task: Some(manager_task),
            operation_task: Some(operation_task),
        }
    }
}

impl<S: SweepStore> World<S> {
    /// The fake clock's reading.
    pub fn now(&self) -> Timestamp {
        Clock::now(&*self.clock)
    }

    /// Moves the fake clock forward by `secs`.
    pub fn advance(&self, secs: i64) {
        self.clock.advance(SignedDuration::from_secs(secs));
    }

    /// Lets every queued task run without moving time.
    pub async fn settle(&self) {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    /// Lets the services run, then waits until the manager has applied everything queued before
    /// the call: a round-trip through its queue.
    ///
    /// Polling the store cannot stand in for this. A commit against SQLite on a loaded machine
    /// lasts far longer than any loop of polls does, so a poll budget expires while the service
    /// is still writing; a round-trip simply takes as long as the commit takes.
    pub async fn let_services_work(&self) {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        let _ = self.manager.tick().await;
    }

    /// Waits until the manager's queue has drained and the operation service has no effect
    /// in flight, so a read of the store is not torn by a commit landing between two of its
    /// queries.
    ///
    /// Each round yields, drains the manager and looks for work in flight. Fifteen quiet
    /// rounds, a second of time apart, also cover a pending verification, whose wait is at
    /// most ten seconds here.
    pub async fn quiesce(&self)
    where
        S: OperationRepo,
    {
        let mut quiet = 0;
        for _ in 0..600 {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            assert!(
                self.manager.tick().await.is_ok(),
                "the manager stopped while waiting for it to go quiet"
            );
            quiet = if self.operation_effects_busy().await {
                0
            } else {
                quiet + 1
            };
            if quiet >= 15 {
                return;
            }
            self.advance(1);
        }
        panic!("the services never went quiet");
    }

    /// Waits until the operation service has nothing left to do at the current fake time: no
    /// operation effect is claimed, and none is pending and due. Time does not move, so an
    /// effect that is only due later (a wake, a verification) is not waited for.
    ///
    /// The scenario calls it after every step. Without it a handler runs beside the next step
    /// and its commits land before or after the step's own depending on the scheduler, which
    /// changes the commit sequence from one run to the next. Waiting for quiet makes the
    /// sequence a function of the scenario alone.
    ///
    /// Returns at once when the process is down: a claimed effect then stays claimed for ever.
    /// Ten quiet rounds in a row are needed, so a runner between a claim and its handler is
    /// not mistaken for an idle one, and a run still out with its agent counts as work; each round reads the store, which takes as long as the
    /// store takes.
    pub async fn operations_quiet(&self)
    where
        S: OperationRepo,
    {
        let mut quiet = 0;
        for _ in 0..5_000 {
            if self.crashing.crashed() {
                return;
            }
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            quiet = if self.operation_effects_busy().await {
                0
            } else {
                quiet + 1
            };
            if quiet >= 10 {
                return;
            }
        }
        panic!("the operation service never went quiet");
    }

    /// Whether an operation effect is claimed or pending and due now, or a run is out with its
    /// agent. A run has no effect of its own while it is out: the dispatch commit completes the
    /// dispatch effect, and the commit that records the run's answer comes later from the
    /// handler that is still waiting for it.
    async fn operation_effects_busy(&self) -> bool
    where
        S: OperationRepo,
    {
        match OperationRepo::dispatched_on(&*self.store, host_id()).await {
            Ok(running) if running.is_empty() => {}
            _ => return true,
        }
        let now = self.now();
        // Pending first: an effect that moves from pending to claimed between the two reads is
        // then seen by the second one.
        for status in [EffectStatus::Pending, EffectStatus::Claimed] {
            let Ok(rows) = EffectRead::effects_with_status(&*self.store, status, 500).await else {
                return true;
            };
            if rows.iter().any(|row| {
                row.aggregate.kind == AggregateKind::Operation
                    && (status == EffectStatus::Claimed || row.not_before <= now)
            }) {
                return true;
            }
        }
        false
    }

    /// Every stored incident, newest first.
    pub async fn incidents(&self) -> Vec<Incident> {
        let mut items = Vec::new();
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got = IncidentRepo::list(&*self.store, &IncidentFilter::default(), &page)
                .await
                .expect("the incidents are readable");
            items.extend(got.items);
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => return items,
            }
        }
    }

    /// Stops the services and waits for their tasks to finish.
    pub async fn shutdown(mut self) {
        self.shutdown.cancel();
        join(self.manager_task.take()).await;
        join(self.operation_task.take()).await;
    }

    /// Lets the restarted services do their start-up work: one tick, and every effect they
    /// have due, until nothing is in flight.
    pub async fn recover(&self) -> Result<(), String>
    where
        S: OperationRepo,
    {
        self.advance(1);
        self.manager
            .tick()
            .await
            .map_err(|err| format!("the restarted manager did not tick: {err}"))?;
        self.quiesce().await;
        Ok(())
    }

    /// Proves the restarted operation service still takes a command: a new operation is
    /// stored and admitted.
    pub async fn assert_operations_work(&self) {
        let id = self
            .operations
            .create(operation_seed(Vec::new()))
            .await
            .expect("the operation service stores a new operation");
        // Only a running (admitted) operation can be paused.
        self.operations
            .pause(id, Actor::System("crash-sweep".to_owned()))
            .await
            .expect("the new operation was admitted");
    }

    /// Proves the restarted services still do their work: one new incident, a new
    /// operation and the invariants again.
    pub async fn assert_services_work(&self)
    where
        S: OperationRepo,
    {
        let before = self.incidents().await;
        let known: Vec<IncidentId> = before.iter().map(Incident::id).collect();
        let signal = firing(
            new_host_id(),
            "net.link",
            Some("eth0"),
            SignalClass::Immediate,
            self.now(),
        );
        self.manager
            .ingest(vec![signal])
            .await
            .expect("the manager stores a signal for a new host");

        let after = self.incidents().await;
        assert_eq!(
            after.len(),
            before.len() + 1,
            "the new host's immediate signal opens exactly one incident"
        );
        assert!(
            after.iter().any(|incident| !known.contains(&incident.id())),
            "the new incident is there"
        );

        self.quiesce().await;
        self.assert_operations_work().await;
        if let Err(violations) = check_invariants(&*self.store).await {
            panic!("invariants after recovery: {violations:#?}");
        }
    }
}

/// Waits for a service task to finish. The shutdown token is already cancelled, so the task
/// ends once the commit it may be in the middle of has landed; how long that takes depends on
/// the store and the machine, so a count of yields cannot bound it. The real-time limit only
/// turns a task that never stops into a failure instead of a hang.
async fn join(task: Option<JoinHandle<()>>) {
    let Some(task) = task else {
        return;
    };
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(60), task).await;
    assert!(stopped.is_ok(), "services did not stop");
}

/// Scripts the agent: connected, announcing every skill, restarting containers and resolving
/// the check.
fn script(link: &FakeAgentLink) {
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    // Every dispatch records the journal it was sent under, so the fixture agent has one.
    link.set_journal_epoch(host_id(), JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
    link.set_skills(host_id(), vec![verified_restart()]);
    link.on_skill(|_, dispatch| {
        if dispatch.skill.as_str() == "restart_failing" {
            Ok(SkillOutput {
                exit_code: Some(3),
                ..ok_output("boom")
            })
        } else {
            Ok(ok_output("restarted nginx"))
        }
    });
    link.on_check(|_, _, _| Ok(check(SignalStatus::Resolved)));
}
