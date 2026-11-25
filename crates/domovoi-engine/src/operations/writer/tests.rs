//! The handler commands, which no caller outside the crate can send: what each commits and
//! what a refusal or a stale claim leaves alone. They run against the real writer over the
//! in-memory store, with the effects claimed by hand the way the runner would claim them.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::ContractId;
use domovoi_core::ports::{
    AggregateKind, AggregateRef, AuditFilter, AuditLog, ClaimToken, EffectKind, EffectQueue,
    EffectRead, EffectRecord, EffectStatus, HostRepo, OperationRepo, Page, Presence,
};
use domovoi_core::{
    Actor, AgentIdentity, CheckKey, ExecutionIntent, HostId, OperationId, ParamKind, ParamName,
    ParamSpec, PinnedSubject, PlanBinding, PlanHash, Risk, StepId, StepKind, StepOutcome,
    TargetKey, UserId,
};
use domovoi_testkit::build::{T0, manifest};
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakeTargetResolver, InMemoryStore,
    ListRedactor, approvals,
};
use jiff::SignedDuration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::commands::DispatchAnswer;
use super::{Held, OperationCommand};
use crate::evidence::Evidence;
use crate::operations::decide::EffectSpec;
use crate::operations::{
    ExecutionRequest, OperationDeps, OperationError, OperationHandle, OperationSeed,
    OperationService, OperationSettings,
};

fn host() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

/// A unit of work over the store that can be told to refuse the next commit that writes
/// anything.
struct FailCommit {
    inner: Arc<InMemoryStore>,
    armed: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl domovoi_core::ports::UnitOfWork for FailCommit {
    async fn commit(
        &self,
        tx: domovoi_core::ports::Transaction,
    ) -> Result<domovoi_core::ports::CommitReceipt, domovoi_core::ports::CommitError> {
        let writes = !tx.writes.is_empty();
        if writes && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(domovoi_core::ports::CommitError::Backend("disk".to_owned()));
        }
        domovoi_core::ports::UnitOfWork::commit(&*self.inner, tx).await
    }
}

struct World {
    commits: Arc<FailCommit>,
    store: Arc<InMemoryStore>,
    clock: Arc<FakeClock>,
    link: Arc<FakeAgentLink>,
    registry: Arc<FakeCapabilityRegistry>,
    handle: OperationHandle,
    shutdown: CancellationToken,
    user: UserId,
}

impl Drop for World {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

async fn world() -> World {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        host(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    let mut skill = manifest("restart_container", Risk::Write).unwrap();
    skill.params = vec![ParamSpec {
        name: ParamName::parse("container").unwrap(),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    link.set_skills(
        host(),
        vec![skill.summary(domovoi_core::ScriptDigest::of_bytes(b"entry"))],
    );
    let mut registered = domovoi_testkit::build::host("nas").unwrap();
    registered.id = host();
    HostRepo::upsert_host(&*store, &registered).await.unwrap();
    enroll(&store, &link, [7; 32]).await;
    let registry = Arc::new(FakeCapabilityRegistry::following(link.clone()));
    let caps = action_caps(link.clone());
    let shutdown = CancellationToken::new();
    let commits = Arc::new(FailCommit {
        inner: store.clone(),
        armed: std::sync::atomic::AtomicBool::new(false),
    });
    let (handle, _task) = OperationService::spawn_bare(
        OperationDeps {
            bus: tokio::sync::broadcast::channel(16).0,
            operations: store.clone(),
            experience: store.clone(),
            uow: commits.clone(),
            effects: store.clone(),
            effect_read: store.clone(),
            advisories: store.clone(),
            incidents: store.clone(),
            hosts: store.clone(),
            evidence: Evidence::new(evidence_caps(link.clone()), clock.clone(), registry.clone()),
            registry: registry.clone(),
            resolver: Arc::new(FakeTargetResolver::new()),
            observer: observer_of(link.clone()),
            journal: caps.journal,
            handoff: caps.handoff,
            rerun: caps.rerun,
            redactor: Arc::new(ListRedactor::new(["hunter2"])),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
            investigate: None,
            autonomy: None,
            contain: None,
        },
        OperationSettings::default(),
    );
    World {
        commits,
        store,
        clock,
        link,
        registry,
        handle,
        shutdown,
        user: UserId::new(),
    }
}

async fn enroll(store: &InMemoryStore, link: &FakeAgentLink, key: [u8; 32]) {
    HostRepo::put_agent(
        store,
        &AgentIdentity {
            agent: link.agent_id(host()),
            host: host(),
            public_key: key,
            version: "0.1.0".to_owned(),
            enrolled_at: T0,
            revoked_at: None,
        },
    )
    .await
    .unwrap();
}

impl World {
    /// A running operation with one proposed restart, and its Execution step.
    async fn proposed(&self) -> (OperationId, StepId) {
        let subject = PinnedSubject {
            host: host(),
            check: CheckKey::parse("container.down").unwrap(),
            target: Some(TargetKey::parse("plex").unwrap()),
        };
        let seed = OperationSeed {
            kind: domovoi_core::OperationKind::Investigate,
            goal: "plex is down".to_owned(),
            pinned: vec![subject],
            incidents: Vec::new(),
            parent: None,
            budgets: None,
            first_steps: Vec::new(),
            effects: Vec::new(),
            actor: Actor::System("tests".to_owned()),
        };
        let op = self.handle.create(seed).await.unwrap();
        let mut args = BTreeMap::new();
        args.insert("container".to_owned(), "plex".to_owned());
        let request = ExecutionRequest {
            intent: ExecutionIntent {
                contract: ContractId::parse("skill.restart_container").unwrap(),
                contract_version: 1,
                host: host(),
                check: CheckKey::parse("container.down").unwrap(),
                target: Some(TargetKey::parse("plex").unwrap()),
                args,
                retry_of: None,
            },
            parent: None,
            caused_by: None,
        };
        let step = self.handle.propose_execution(op, request).await.unwrap();
        (op, step)
    }

    /// A running operation with one approved restart, and its Execution step.
    async fn approved(&self) -> (OperationId, StepId) {
        let (op, step) = self.proposed().await;
        let plan = self.plan(op, step).await;
        self.handle
            .authorize(
                op,
                step,
                approvals::authorize(approvals::human(self.user)),
                PlanBinding::Full(plan),
                approvals::provenance(),
                None,
            )
            .await
            .unwrap();
        (op, step)
    }

    async fn plan(&self, op: OperationId, step: StepId) -> PlanHash {
        let snapshot = self.store.load(op).await.unwrap().unwrap();
        snapshot.execution_of(step).unwrap().value.plan_hash
    }

    /// The live session, as the dispatch reads it.
    fn live(&self) -> domovoi_core::ports::AgentSnapshot {
        observer_of(self.link.clone())
            .observe_snapshot(host())
            .unwrap()
    }

    /// Claims the reconcile effect once its delay has passed.
    async fn claim_reconcile(&self, op: OperationId) -> (Held, EffectRecord) {
        self.clock.advance(SignedDuration::from_secs(61));
        self.claim(op, EffectKind::Reconcile).await
    }

    async fn audited(&self, kind: &str) -> bool {
        let filter = AuditFilter {
            kind: Some(kind.to_owned()),
            ..AuditFilter::default()
        };
        let page = AuditLog::list(&*self.store, &filter, &Page::first(1))
            .await
            .unwrap();
        !page.items.is_empty()
    }

    fn now(&self) -> jiff::Timestamp {
        domovoi_core::ports::Clock::now(&*self.clock)
    }

    /// Claims the one due effect of `kind` for `op`, as the runner would.
    async fn claim(&self, op: OperationId, kind: EffectKind) -> (Held, EffectRecord) {
        let batch = EffectQueue::claim_due(
            &*self.store,
            "test-runner",
            &[kind],
            &[AggregateKind::Operation],
            self.now(),
            SignedDuration::from_mins(5),
            50,
        )
        .await
        .unwrap();
        let claimed = batch
            .claimed
            .into_iter()
            .find(|c| c.record.aggregate == AggregateRef::operation(op))
            .expect("a due effect of that kind");
        (
            Held {
                effect: claimed.record.id,
                claim: claimed.token,
            },
            claimed.record,
        )
    }

    async fn send<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, OperationError>>) -> OperationCommand,
    ) -> Result<T, OperationError> {
        let (reply, answer) = oneshot::channel();
        self.handle.shared.writer.send(make(reply)).await.unwrap();
        answer.await.unwrap()
    }

    /// Sends a dispatch for `step` as the execute step would, with the stored plan.
    async fn dispatch(
        &self,
        op: OperationId,
        step: StepId,
        plan: PlanHash,
    ) -> Result<DispatchAnswer, OperationError> {
        let (held, _) = self.claim(op, EffectKind::Dispatch).await;
        let live = self.live();
        self.send(|reply| OperationCommand::Dispatch {
            held,
            operation: op,
            step,
            plan,
            session: live.session,
            epoch: live.journal_epoch,
            reply,
        })
        .await
    }

    async fn effects(&self, op: OperationId, status: EffectStatus) -> Vec<EffectRecord> {
        EffectRead::effects_with_status(&*self.store, status, 500)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.aggregate == AggregateRef::operation(op))
            .collect()
    }

    async fn outcome(&self, op: OperationId, step: StepId) -> StepOutcome {
        let snapshot = self.store.load(op).await.unwrap().unwrap();
        snapshot.step(step).unwrap().progress.outcome
    }

    async fn reason(&self, op: OperationId, step: StepId) -> Option<String> {
        let snapshot = self.store.load(op).await.unwrap().unwrap();
        snapshot.step(step).unwrap().progress.reason.clone()
    }
}

#[tokio::test]
async fn dispatch_records_the_epoch_and_completes_its_effect_in_one_commit() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;

    let answer = w.dispatch(op, step, plan).await.unwrap();

    assert!(matches!(answer, DispatchAnswer::Dispatched(_)));
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Running
    );
    let row = &snapshot.execution_of(step).unwrap().value;
    let (epoch, at) = row.dispatched.expect("the hand-off is recorded on the row");
    assert_eq!(epoch, w.live().journal_epoch);
    assert_eq!(at, w.now());
    assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
    let done = w.effects(op, EffectStatus::Done).await;
    assert!(done.iter().any(|e| e.kind == "dispatch"));
    let reconcile = w.effects(op, EffectStatus::Pending).await;
    let reconcile = reconcile.iter().find(|e| e.kind == "reconcile").unwrap();
    assert_eq!(
        reconcile.not_before,
        w.now() + SignedDuration::from_secs(60)
    );
}

/// A refusal reason and the change to the world that causes it.
type Case = (&'static str, fn(&World));

#[tokio::test]
async fn dispatch_is_refused_for_every_change_since_approval() {
    let cases: [Case; 4] = [
        ("the agent session changed before dispatch", |w| {
            w.link.new_session(host());
        }),
        ("the skill is disabled on this host", |w| {
            w.registry.disable(
                host(),
                domovoi_core::SkillName::parse("restart_container").unwrap(),
            );
        }),
        ("the agent session changed before dispatch", |w| {
            w.link
                .set_presence(host(), Presence::Disconnected { last_seen: None });
        }),
        ("the host's enrolled agent changed before dispatch", |w| {
            w.link.set_agent(host(), domovoi_core::AgentId::new());
        }),
    ];
    for (reason, change) in cases {
        let w = world().await;
        let (op, step) = w.approved().await;
        let plan = w.plan(op, step).await;
        let live = w.live();
        let (held, _) = w.claim(op, EffectKind::Dispatch).await;
        change(&w);

        let answer = w
            .send(|reply| OperationCommand::Dispatch {
                held,
                operation: op,
                step,
                plan,
                session: live.session,
                epoch: live.journal_epoch,
                reply,
            })
            .await
            .unwrap();

        assert_eq!(
            answer,
            DispatchAnswer::Settled(StepOutcome::Refused),
            "{reason}"
        );
        assert_eq!(w.reason(op, step).await.as_deref(), Some(reason));
        assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
    }
}

#[tokio::test]
async fn dispatch_with_another_plan_hash_is_refused() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let other: PlanHash = "ab".repeat(32).parse().unwrap();

    let answer = w.dispatch(op, step, other).await.unwrap();

    assert_eq!(answer, DispatchAnswer::Settled(StepOutcome::Refused));
    assert_eq!(
        w.reason(op, step).await.as_deref(),
        Some("the plan changed after approval")
    );
    assert!(w.audited("execution.refused").await);
}

#[tokio::test]
async fn dispatch_after_the_window_closed_settles_not_run() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.clock.advance(SignedDuration::from_hours(25));

    let answer = w.dispatch(op, step, plan).await.unwrap();

    assert_eq!(answer, DispatchAnswer::Settled(StepOutcome::NotRun));
}

#[tokio::test]
async fn dispatch_to_an_unavailable_capability_settles_unavailable() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.registry.set_availability(
        host(),
        &ContractId::parse("skill.restart_container").unwrap(),
        domovoi_core::capability::Availability::NotInstalled,
    );

    let answer = w.dispatch(op, step, plan).await.unwrap();

    assert_eq!(answer, DispatchAnswer::Settled(StepOutcome::Unavailable));
}

#[tokio::test]
async fn a_paused_operation_keeps_its_dispatch_for_later() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.handle.pause(op, Actor::User(w.user)).await.unwrap();

    let refused = w.dispatch(op, step, plan).await;

    assert!(matches!(refused, Err(OperationError::Decide(_))));
    assert_eq!(w.outcome(op, step).await, StepOutcome::Pending);
    let pending = w.effects(op, EffectStatus::Pending).await;
    assert!(
        pending.iter().any(|e| e.kind == "dispatch"),
        "rescheduled, not lost"
    );
}

#[tokio::test]
async fn a_stale_claim_changes_nothing() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();
    let before = w.store.load(op).await.unwrap().unwrap();
    let (held, _) = w.claim_reconcile(op).await;
    let stale = Held {
        effect: held.effect,
        claim: ClaimToken::new(),
    };

    let refused = w
        .send(|reply| OperationCommand::Settle {
            held: Some(stale),
            operation: op,
            step,
            outcome: StepOutcome::Succeeded,
            reason: None,
            output: None,
            verify_after: None,
            reply,
        })
        .await;

    assert_eq!(refused, Err(OperationError::StaleEffect));
    assert_eq!(w.store.load(op).await.unwrap().unwrap(), before);
    assert_eq!(
        w.effects(op, EffectStatus::Claimed).await.len(),
        1,
        "the real claim is intact"
    );
}

#[tokio::test]
async fn settling_a_success_creates_the_verification_and_redacts_the_output() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();

    w.send(|reply| OperationCommand::Settle {
        held: None,
        operation: op,
        step,
        outcome: StepOutcome::Succeeded,
        reason: None,
        output: Some("restarted with token hunter2".to_owned()),
        verify_after: Some(SignedDuration::from_secs(30)),
        reply,
    })
    .await
    .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let record = snapshot.step(step).unwrap();
    assert_eq!(record.progress.outcome, StepOutcome::Succeeded);
    assert_eq!(
        record.progress.output.as_deref(),
        Some("restarted with token [REDACTED]")
    );
    assert!(
        snapshot
            .execution_of(step)
            .unwrap()
            .value
            .settled_at
            .is_some()
    );
    assert!(
        snapshot
            .steps
            .iter()
            .any(|r| r.step.kind == StepKind::Verification)
    );
    let pending = w.effects(op, EffectStatus::Pending).await;
    assert!(pending.iter().any(|e| e.kind == "verify"));
    let journal =
        domovoi_core::ports::JournalRead::entries_for(&*w.store, &AggregateRef::operation(op), 500)
            .await
            .unwrap();
    assert!(
        !serde_json::to_string(
            &journal
                .iter()
                .map(|e| e.payload.clone())
                .collect::<Vec<_>>()
        )
        .unwrap()
        .contains("restarted")
    );
}

#[tokio::test]
async fn an_unknown_outcome_pauses_the_operation() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();

    w.send(|reply| OperationCommand::Settle {
        held: None,
        operation: op,
        step,
        outcome: StepOutcome::OutcomeUnknown,
        reason: Some("the agent restarted".to_owned()),
        output: None,
        verify_after: None,
        reply,
    })
    .await
    .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(snapshot.operation.status.name(), "paused");
}

#[tokio::test]
async fn deferring_completes_the_effect_and_records_the_wait_once() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let (held, record) = w.claim(op, EffectKind::Dispatch).await;
    let successor = |key: &str| EffectSpec {
        id: domovoi_core::EffectId::new(),
        kind: EffectKind::Wake,
        payload: serde_json::json!({ "v": 1, "operation": op.to_string(), "step": step.to_string(), "purpose": "retry_dispatch" }),
        not_before: w.now() + SignedDuration::from_secs(5),
        max_attempts: 3,
        dedup_key: Some(key.to_owned()),
    };

    w.send(|reply| OperationCommand::Defer {
        held,
        operation: op,
        step: Some(step),
        successor: Box::new(successor("wake:retry:1")),
        waiting: true,
        reply,
    })
    .await
    .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let since = snapshot.execution_of(step).unwrap().value.waiting_since;
    assert_eq!(since, Some(w.now()));
    assert_eq!(snapshot.operation.status.name(), "waiting_agent");
    assert!(
        EffectRead::effect(&*w.store, record.id)
            .await
            .unwrap()
            .unwrap()
            .status
            == "done"
    );
    let wakes = w.effects(op, EffectStatus::Pending).await;
    assert!(
        wakes
            .iter()
            .any(|e| e.dedup_key.as_deref() == Some("wake:retry:1"))
    );

    w.clock.advance(SignedDuration::from_secs(10));
    let (again, _) = w.claim(op, EffectKind::Wake).await;
    w.send(|reply| OperationCommand::Defer {
        held: again,
        operation: op,
        step: Some(step),
        successor: Box::new(successor("wake:retry:1")),
        waiting: true,
        reply,
    })
    .await
    .unwrap();
    let after = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(
        after.execution_of(step).unwrap().value.waiting_since,
        since,
        "the wait starts once"
    );
}

#[tokio::test]
async fn expiring_closes_the_approval_and_the_execution_with_the_wake() {
    let w = world().await;
    let (op, _) = w.proposed().await;
    w.clock.advance(SignedDuration::from_hours(25));
    let (held, _) = w.claim(op, EffectKind::Wake).await;
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let step = snapshot
        .steps
        .iter()
        .find(|r| r.step.kind == StepKind::Execution)
        .unwrap()
        .step
        .id;

    w.send(|reply| OperationCommand::Expire {
        held,
        operation: op,
        step,
        reply,
    })
    .await
    .unwrap();

    assert_eq!(w.outcome(op, step).await, StepOutcome::NotRun);
    assert_eq!(
        w.reason(op, step).await.as_deref(),
        Some("the approval expired before the action was dispatched")
    );
}

#[tokio::test]
async fn a_late_result_after_cancel_is_recorded_and_still_reconciled() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();

    w.handle
        .cancel(op, Actor::User(w.user), "stop".to_owned())
        .await
        .unwrap();

    assert_eq!(
        w.outcome(op, step).await,
        StepOutcome::Running,
        "not recalled"
    );
    let pending = w.effects(op, EffectStatus::Pending).await;
    assert_eq!(
        pending.iter().filter(|e| e.kind == "reconcile").count(),
        1,
        "the cancellation dropped the old reconcile and queued a fresh one"
    );
    w.send(|reply| OperationCommand::Settle {
        held: None,
        operation: op,
        step,
        outcome: StepOutcome::Succeeded,
        reason: None,
        output: None,
        verify_after: Some(SignedDuration::from_secs(30)),
        reply,
    })
    .await
    .unwrap();
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(snapshot.operation.status.name(), "cancelled");
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Succeeded
    );
    assert!(
        snapshot
            .steps
            .iter()
            .all(|r| r.step.kind != StepKind::Verification),
        "a late success of a cancelled operation starts no verification"
    );
}

#[tokio::test]
async fn finishing_an_effect_completes_it_and_a_stale_claim_is_refused() {
    let w = world().await;
    let (op, _) = w.approved().await;
    let (held, record) = w.claim(op, EffectKind::Dispatch).await;
    let stale = Held {
        effect: held.effect,
        claim: ClaimToken::new(),
    };

    let refused = w
        .send(|reply| OperationCommand::FinishEffect { held: stale, reply })
        .await;
    assert_eq!(refused, Err(OperationError::StaleEffect));

    w.send(|reply| OperationCommand::FinishEffect { held, reply })
        .await
        .unwrap();
    let stored = EffectRead::effect(&*w.store, record.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, "done");
}

#[tokio::test]
async fn reconciling_a_host_queues_one_effect_per_dispatched_execution() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();
    // The dispatch already queued one reconcile; the host's hello must not add a second.
    for _ in 0..2 {
        let (reply, answer) = oneshot::channel();
        w.handle
            .shared
            .writer
            .send(OperationCommand::ReconcileHost {
                host: host(),
                reply,
            })
            .await
            .unwrap();
        answer.await.unwrap();
    }
    let reconciles: Vec<_> = w
        .effects(op, EffectStatus::Pending)
        .await
        .into_iter()
        .filter(|e| e.kind == "reconcile")
        .collect();
    assert_eq!(reconciles.len(), 1);
}

#[tokio::test]
async fn a_step_the_handler_starts_and_finishes_is_recorded_with_its_effect() {
    let w = world().await;
    let (op, _) = w.proposed().await;
    let step = StepId::new();
    let evidence = crate::operations::NewStep {
        id: step,
        kind: StepKind::EvidenceRequest,
        phase: "evidence".to_owned(),
        parent: None,
        intent: domovoi_core::StepIntent::EvidenceRequest {
            host: host(),
            tool: "docker_ps".to_owned(),
            args: serde_json::json!({}),
        },
    };
    w.handle
        .propose(op, vec![evidence], Vec::new())
        .await
        .unwrap();
    let (held, _) = w.claim(op, EffectKind::CollectEvidence).await;

    w.send(|reply| OperationCommand::StartStep {
        held,
        operation: op,
        step,
        reply,
    })
    .await
    .unwrap();
    assert_eq!(w.outcome(op, step).await, StepOutcome::Running);
    assert_eq!(
        w.effects(op, EffectStatus::Claimed).await.len(),
        1,
        "starting keeps the effect, which also completes with the finish"
    );
    w.send(|reply| OperationCommand::FinishStep {
        held,
        operation: op,
        step,
        outcome: StepOutcome::Succeeded,
        summary: Some("two containers, hunter2".to_owned()),
        reason: None,
        output: Some("hunter2".to_owned()),
        reply,
    })
    .await
    .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let record = snapshot.step(step).unwrap();
    assert_eq!(record.progress.outcome, StepOutcome::Succeeded);
    assert_eq!(
        record.progress.summary.as_deref(),
        Some("two containers, [REDACTED]")
    );
    assert_eq!(record.progress.output.as_deref(), Some("[REDACTED]"));
    assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
}

#[tokio::test]
async fn a_verification_answer_finishes_the_operation_with_its_effect() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();
    w.send(|reply| OperationCommand::Settle {
        held: None,
        operation: op,
        step,
        outcome: StepOutcome::Succeeded,
        reason: None,
        output: None,
        verify_after: Some(SignedDuration::from_secs(30)),
        reply,
    })
    .await
    .unwrap();
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let verification = snapshot
        .steps
        .iter()
        .find(|r| r.step.kind == StepKind::Verification)
        .unwrap()
        .step
        .id;
    w.clock.advance(SignedDuration::from_secs(31));
    let (held, _) = w.claim(op, EffectKind::Verify).await;

    w.send(|reply| OperationCommand::Verified {
        held,
        operation: op,
        step: verification,
        passed: true,
        summary: "the container is up".to_owned(),
        reply,
    })
    .await
    .unwrap();

    assert_eq!(w.outcome(op, verification).await, StepOutcome::Succeeded);
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(snapshot.operation.status.name(), "succeeded");
    assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
    let pending = w.effects(op, EffectStatus::Pending).await;
    assert!(
        pending
            .iter()
            .all(|effect| effect.kind == EffectKind::DraftResolution.as_str()),
        "the end cancels the rest and leaves only the summary draft"
    );
}

#[tokio::test]
async fn the_wall_time_wake_ticks_the_budget_and_pauses_past_the_limit() {
    let w = world().await;
    let (op, _) = w.proposed().await;
    w.clock.advance(SignedDuration::from_hours(73));
    let (held, _) = w.claim(op, EffectKind::Wake).await;
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(snapshot.operation.status.name(), "waiting_approval");

    w.send(|reply| OperationCommand::Tick {
        held,
        operation: op,
        reply,
    })
    .await
    .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(snapshot.operation.status.name(), "paused");
    assert_eq!(snapshot.operation.budgets.used.wall_seconds, 73 * 3600);
}

#[tokio::test]
async fn a_secret_in_a_json_output_key_is_redacted_and_the_document_stays_valid() {
    let w = world().await;
    let (op, step) = w.approved().await;
    let plan = w.plan(op, step).await;
    w.dispatch(op, step, plan).await.unwrap();

    w.send(|reply| OperationCommand::Settle {
        held: None,
        operation: op,
        step,
        outcome: StepOutcome::Succeeded,
        reason: None,
        output: Some(r#"{"token hunter2":"ok","nested":{"note":"token hunter2"}}"#.to_owned()),
        verify_after: None,
        reply,
    })
    .await
    .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let output = snapshot
        .step(step)
        .unwrap()
        .progress
        .output
        .clone()
        .unwrap();
    assert!(!output.contains("hunter2"), "{output}");
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["token [REDACTED]"], "ok");
    assert_eq!(parsed["nested"]["note"], "token [REDACTED]");
}

/// A running operation with a running reasoning turn, and the claimed effect the turn answers.
async fn turn_with_effect(w: &World) -> (OperationId, StepId, Held) {
    let (op, _) = w.approved().await;
    let turn = StepId::new();
    let step = crate::operations::NewStep {
        id: turn,
        kind: StepKind::ReasoningTurn,
        phase: "investigate".to_owned(),
        parent: None,
        intent: domovoi_core::StepIntent::ReasoningTurn {
            context_refs: Vec::new(),
        },
    };
    w.handle.propose(op, vec![step], Vec::new()).await.unwrap();
    // Any claimed effect of the operation stands in for the `call_model` the turn answers.
    let (held, _) = w.claim(op, EffectKind::Dispatch).await;
    (op, turn, held)
}

fn read(tool: &str, outcome: StepOutcome) -> crate::operations::RecordedStep {
    crate::operations::RecordedStep {
        id: StepId::new(),
        host: host(),
        tool: tool.to_owned(),
        args: serde_json::json!({}),
        outcome,
        summary: Some("two containers, hunter2".to_owned()),
        reason: None,
        output: Some("hunter2".to_owned()),
    }
}

fn turn_end(
    op: OperationId,
    turn: StepId,
    held: Held,
    evidence: Vec<crate::operations::RecordedStep>,
) -> Box<super::TurnEnd> {
    Box::new(super::TurnEnd {
        held,
        operation: op,
        turn: super::Conclusion {
            step: turn,
            outcome: StepOutcome::Succeeded,
            summary: Some("done".to_owned()),
            reason: None,
            output: Some("{}".to_owned()),
            usage: None,
        },
        evidence,
        successors: Vec::new(),
    })
}

#[tokio::test]
async fn finishing_a_turn_records_its_reads_and_completes_the_effect_in_one_commit() {
    let w = world().await;
    let (op, turn, held) = turn_with_effect(&w).await;
    let reads = vec![
        read("docker_ps", StepOutcome::Succeeded),
        read("docker_ps", StepOutcome::Refused),
    ];
    let ids: Vec<StepId> = reads.iter().map(|r| r.id).collect();
    let end = turn_end(op, turn, held, reads);
    w.send(|reply| OperationCommand::FinishTurn { end, reply })
        .await
        .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(
        snapshot.step(turn).unwrap().progress.outcome,
        StepOutcome::Succeeded
    );
    for id in &ids {
        let record = snapshot.step(*id).unwrap();
        assert_eq!(record.step.parent, Some(turn));
        assert_eq!(
            record.progress.summary.as_deref(),
            Some("two containers, [REDACTED]")
        );
        assert_eq!(record.progress.output.as_deref(), Some("[REDACTED]"));
    }
    assert_eq!(snapshot.operation.budgets.used.evidence_requests, 1);
    assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
}

#[tokio::test]
async fn a_read_that_does_not_validate_is_dropped_and_the_report_is_not_lost() {
    let w = world().await;
    let (op, turn, held) = turn_with_effect(&w).await;
    let reads = vec![read("rm_rf", StepOutcome::Succeeded)];
    let end = turn_end(op, turn, held, reads);
    w.send(|reply| OperationCommand::FinishTurn { end, reply })
        .await
        .unwrap();

    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let record = snapshot.step(turn).unwrap();
    assert_eq!(record.progress.outcome, StepOutcome::Succeeded);
    assert_eq!(
        record.progress.reason.as_deref(),
        Some("its reads were not recorded")
    );
    assert!(
        snapshot
            .steps
            .iter()
            .all(|r| r.step.kind != StepKind::EvidenceRequest)
    );
    assert!(w.effects(op, EffectStatus::Claimed).await.is_empty());
}

#[tokio::test]
async fn evidence_steps_and_the_turn_commit_atomically() {
    let w = world().await;
    let (op, turn, held) = turn_with_effect(&w).await;
    let end = turn_end(
        op,
        turn,
        held,
        vec![read("docker_ps", StepOutcome::Succeeded)],
    );
    w.commits
        .armed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let failed = w
        .send(|reply| OperationCommand::FinishTurn {
            end: end.clone(),
            reply,
        })
        .await;

    assert!(failed.is_err());
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    assert_eq!(
        snapshot.step(turn).unwrap().progress.outcome,
        StepOutcome::Running
    );
    assert!(
        snapshot
            .steps
            .iter()
            .all(|r| r.step.kind != StepKind::EvidenceRequest)
    );
    assert_eq!(w.effects(op, EffectStatus::Claimed).await.len(), 1);
    // The re-issued call records once.
    w.send(|reply| OperationCommand::FinishTurn { end, reply })
        .await
        .unwrap();
    let snapshot = w.store.load(op).await.unwrap().unwrap();
    let count = snapshot
        .steps
        .iter()
        .filter(|r| r.step.kind == StepKind::EvidenceRequest)
        .count();
    assert_eq!(count, 1);
}
