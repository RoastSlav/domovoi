//! Driving and reading an investigation world: time, stored state, events and effects.

use domovoi_core::ports::{
    AdvisoryRepo, AggregateRef, EffectKind, EffectRead, EffectRecord, EffectStatus, IncidentRepo,
    IncidentTimelineEntry, OperationRepo, Page,
};
use domovoi_core::{
    ActionRequest, CheckKey, Evidence, Incident, Operation, OperationId, OperationSnapshot,
    StepKind, StepRecord, TargetKey, TriageOutcome,
};
use domovoi_engine::operations::OperationService;
use domovoi_engine::{DomainEvent, EventKind};
use domovoi_testkit::build::{T0, open_incident};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::harness::host_id;
use crate::inv::{Inv, add_signal, lock};

impl Inv {
    /// An open `docker.container` incident on `target`, with one signal carrying `evidence`.
    pub async fn add_incident(&self, target: &str, evidence: Evidence) -> Incident {
        let incident = open_incident(host_id(), "docker.container", Some(target)).unwrap();
        IncidentRepo::insert(&*self.store, &incident).await.unwrap();
        add_signal(&self.store, &incident, evidence, self.now()).await;
        incident
    }

    /// Resolves `incident` in the store, as a cleared check does.
    pub async fn resolve(&self, incident: &Incident) {
        let mut found = IncidentRepo::get(&*self.store, incident.id())
            .await
            .unwrap()
            .unwrap();
        domovoi_core::apply(
            &mut found,
            domovoi_core::IncidentEvent::Resolved {
                observed_at: self.now(),
            },
            &domovoi_core::IncidentPolicy::default(),
            self.now(),
        )
        .unwrap();
        IncidentRepo::update(&*self.store, &found).await.unwrap();
    }

    /// Lets queued work run without moving time.
    pub async fn settle(&self) {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    /// The fake clock's time.
    pub fn now(&self) -> Timestamp {
        domovoi_core::ports::Clock::now(&*self.clock)
    }

    /// Moves the clock on and lets the services react.
    pub async fn jump(&self, seconds: i64) {
        self.clock.advance(SignedDuration::from_secs(seconds));
        self.settle().await;
    }

    /// Starts (or finds) the investigation of the world's incident.
    pub async fn start(&self) -> OperationId {
        self.ops.investigate(&self.incident, false).await.unwrap()
    }

    /// The operation as stored.
    pub async fn load(&self, id: OperationId) -> OperationSnapshot {
        OperationRepo::load(&*self.store, id)
            .await
            .unwrap()
            .unwrap()
    }

    /// Every operation about `incident`.
    pub async fn operations_of(&self, incident: &Incident) -> Vec<Operation> {
        OperationRepo::for_incident(&*self.store, incident.id())
            .await
            .unwrap()
    }

    /// Lets the services work, advancing the clock `step` seconds per round, until `done`
    /// holds for the stored operation.
    pub async fn until_stepping(
        &self,
        op: OperationId,
        step: i64,
        done: impl Fn(&OperationSnapshot) -> bool,
    ) -> OperationSnapshot {
        for _ in 0..4000 {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            let snapshot = self.load(op).await;
            if done(&snapshot) {
                return snapshot;
            }
            self.clock.advance(SignedDuration::from_secs(step));
        }
        let last = self.load(op).await;
        panic!(
            "the operation never reached the expected state; it is {:?} with {:#?}",
            last.operation.status,
            last.steps
                .iter()
                .map(|r| (r.step.kind, r.progress.outcome, r.progress.reason.clone()))
                .collect::<Vec<_>>()
        );
    }

    /// `until_stepping` a second per round.
    pub async fn until(
        &self,
        op: OperationId,
        done: impl Fn(&OperationSnapshot) -> bool,
    ) -> OperationSnapshot {
        self.until_stepping(op, 1, done).await
    }

    /// Waits until the operation is in a terminal status.
    pub async fn until_closed(&self, op: OperationId) -> OperationSnapshot {
        self.until(op, |s| s.operation.status.is_terminal()).await
    }

    /// The `kind` entries of the incident's timeline.
    pub async fn timeline(&self, incident: &Incident) -> Vec<IncidentTimelineEntry> {
        IncidentRepo::timeline(&*self.store, incident.id())
            .await
            .unwrap()
    }

    /// How many timeline entries of `kind` the incident has.
    pub async fn count(&self, incident: &Incident, kind: &str) -> usize {
        self.timeline(incident)
            .await
            .iter()
            .filter(|entry| entry.kind == kind)
            .count()
    }

    /// Every event published so far, oldest first.
    pub fn events(&self) -> Vec<DomainEvent> {
        let mut seen = lock(&self.seen);
        let mut receiver = lock(&self.events);
        while let Ok(event) = receiver.try_recv() {
            seen.push(event);
        }
        seen.clone()
    }

    /// The outcomes attached so far with the chat tokens each came with.
    pub fn completed(&self) -> Vec<(TriageOutcome, usize)> {
        self.events()
            .into_iter()
            .filter_map(|event| match event.kind {
                EventKind::TriageCompleted { outcome, tokens } => Some((outcome, tokens.len())),
                _ => None,
            })
            .collect()
    }

    /// The outcome attached last.
    pub fn last_outcome(&self) -> TriageOutcome {
        self.completed()
            .pop()
            .map(|(outcome, _)| outcome)
            .expect("an outcome was attached")
    }

    /// Effects of `op` in `status`, of `kind` when given.
    pub async fn effects(
        &self,
        op: OperationId,
        status: EffectStatus,
        kind: Option<EffectKind>,
    ) -> Vec<EffectRecord> {
        EffectRead::effects_with_status(&*self.store, status, 500)
            .await
            .unwrap()
            .into_iter()
            .filter(|effect| effect.aggregate == AggregateRef::operation(op))
            .filter(|effect| kind.is_none_or(|kind| effect.kind == kind.as_str()))
            .collect()
    }

    /// The effects of `kind` that belong to `op`, whatever their status.
    pub async fn all_effects(&self, op: OperationId, kind: EffectKind) -> Vec<EffectRecord> {
        let mut found = Vec::new();
        for status in [
            EffectStatus::Pending,
            EffectStatus::Claimed,
            EffectStatus::Done,
            EffectStatus::Dead,
        ] {
            found.extend(self.effects(op, status, Some(kind)).await);
        }
        found
    }

    /// The legacy action rows.
    pub async fn actions(&self) -> Vec<ActionRequest> {
        domovoi_core::ports::ActionRepo::list(
            &*self.store,
            &domovoi_core::ports::ActionFilter::default(),
            &Page::first(100),
        )
        .await
        .unwrap()
        .items
    }

    /// The open advisories.
    pub async fn advisories(&self) -> Vec<domovoi_core::Advisory> {
        AdvisoryRepo::list_open(&*self.store).await.unwrap()
    }

    /// Stops the operation service and starts another over the same store, as a hub restart
    /// does.
    pub async fn restart(&mut self) {
        self.ops_shutdown.cancel();
        if let Some(task) = self.ops_task.take() {
            let _ = task.await;
        }
        self.ops_shutdown = CancellationToken::new();
        self.deps.shutdown = self.ops_shutdown.clone();
        let (ops, task) = OperationService::spawn(self.deps.clone(), self.settings.clone());
        self.ops = ops;
        self.ops_task = Some(task);
        self.settle().await;
    }

    /// Approves `step` as the world's user, bound to the stored plan.
    pub async fn approve(
        &self,
        op: OperationId,
        step: domovoi_core::StepId,
    ) -> Result<(), domovoi_engine::operations::OperationError> {
        let binding = domovoi_core::PlanBinding::Full(
            self.load(op)
                .await
                .execution_of(step)
                .unwrap()
                .value
                .plan_hash,
        );
        self.ops
            .authorize(
                op,
                step,
                domovoi_testkit::approvals::authorize(domovoi_testkit::approvals::human(self.user)),
                binding,
                domovoi_testkit::approvals::provenance(),
            )
            .await
    }

    /// The skill requests the fake agent received.
    pub fn skill_requests(&self) -> usize {
        self.link
            .requests()
            .into_iter()
            .filter(|request| matches!(request, domovoi_testkit::link::LinkRequest::Skill { .. }))
            .count()
    }
}

/// The reasoning turns of `snapshot`, in ordinal order.
pub fn turns(snapshot: &OperationSnapshot) -> Vec<&StepRecord> {
    of_kind(snapshot, StepKind::ReasoningTurn)
}

/// The steps of `kind`, in ordinal order.
pub fn of_kind(snapshot: &OperationSnapshot, kind: StepKind) -> Vec<&StepRecord> {
    snapshot
        .steps
        .iter()
        .filter(|record| record.step.kind == kind)
        .collect()
}

/// The `kind` of a turn's stored output.
pub fn output_kind(turn: &StepRecord) -> String {
    let output: Value = serde_json::from_str(turn.progress.output.as_deref().unwrap_or("{}"))
        .unwrap_or(Value::Null);
    output["kind"].as_str().unwrap_or("").to_owned()
}

/// A target key, for incidents on other containers.
pub fn target(name: &str) -> TargetKey {
    TargetKey::parse(name).unwrap()
}

/// The check every test incident is about.
pub fn check() -> CheckKey {
    CheckKey::parse("docker.container").unwrap()
}

impl Inv {
    /// Starts `investigate_and_wait` on a task, so the test can drive the clock meanwhile.
    pub fn spawn_wait(
        &self,
    ) -> tokio::task::JoinHandle<Result<TriageOutcome, domovoi_engine::operations::InvestigateError>>
    {
        let ops = self.ops.clone();
        let incident = self.incident.clone();
        tokio::spawn(async move { ops.investigate_and_wait(&incident, false).await })
    }
}

impl Inv {
    /// Delivers the first effect of `kind` that `op` has had once more, as a runner does
    /// after a crash between a handler's work and its commit: a new effect row with the same
    /// payload. Returns once the runner is done with it.
    pub async fn redeliver(&self, op: OperationId, kind: EffectKind) -> domovoi_core::EffectId {
        use domovoi_core::ports::{EffectDraft, Transaction, UnitOfWork};
        let original = self
            .all_effects(op, kind)
            .await
            .into_iter()
            .next()
            .expect("the effect ran once");
        let id = domovoi_core::EffectId::new();
        let draft = EffectDraft {
            id,
            kind,
            aggregate: AggregateRef::operation(op),
            payload: original.payload.clone(),
            not_before: T0,
            max_attempts: original.max_attempts,
            dedup_key: None,
            created_at: self.now(),
        };
        UnitOfWork::commit(&*self.store, Transaction::new().effect(draft))
            .await
            .unwrap();
        for _ in 0..400 {
            self.settle().await;
            let stored = EffectRead::effect(&*self.store, id).await.unwrap().unwrap();
            if stored.status == EffectStatus::Done.as_str() {
                return id;
            }
            self.clock.advance(SignedDuration::from_secs(1));
        }
        panic!("the redelivered effect never finished");
    }
}
