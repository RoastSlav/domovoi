//! Builders for the operation tests: a counter id source and a fixture that applies each
//! decision to its state, the way a writer would after a commit.

use std::collections::BTreeMap;

use domovoi_core::{
    Actor, BudgetLimits, Budgets, CheckKey, ContractId, EffectId, ExecutionIntent, HostId,
    JournalEpoch, Operation, OperationId, OperationKind, OperationSnapshot, OperationStatus,
    PinnedSubject, PlanBinding, PlanHash, StepId, StepIntent, StepOutcome, TargetKey, UserId,
};
use domovoi_engine::operations::{
    DecideError, Decision, ExecutionProposal, IdSource, NewStep, OperationEvent, OperationState,
    decide::decide,
};
use domovoi_testkit::approvals;
use domovoi_testkit::build::T0;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

/// Ids counted up from a fixed base, so decisions are repeatable.
#[derive(Clone)]
pub struct CounterIds(u128);

impl CounterIds {
    pub fn new() -> Self {
        Self(0)
    }
}

impl IdSource for CounterIds {
    fn step(&mut self) -> StepId {
        self.0 += 1;
        StepId::from_uuid(Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0000 + self.0,
        ))
    }

    fn effect(&mut self) -> EffectId {
        self.0 += 1;
        EffectId::from_uuid(Uuid::from_u128(
            0x0191_0000_0000_7000_8000_0000_0000_0000 + self.0,
        ))
    }
}

/// A plan hash of one repeated byte.
pub fn hash(byte: u8) -> PlanHash {
    hex::encode([byte; 32]).parse().unwrap()
}

/// An operation, its ids and a clock, with `event` applying each decision.
pub struct Fixture {
    pub state: OperationState,
    pub ids: CounterIds,
    pub now: Timestamp,
    pub subject: PinnedSubject,
}

impl Fixture {
    /// A queued operation pinned to one container check.
    pub fn new(limits: BudgetLimits) -> Self {
        let subject = PinnedSubject {
            host: HostId::new(),
            check: CheckKey::parse("container.down").unwrap(),
            target: Some(TargetKey::parse("plex").unwrap()),
        };
        let operation = Operation::queued(
            OperationId::new(),
            OperationKind::Investigate,
            "find out why plex is down",
            vec![subject.clone()],
            None,
            Budgets::new(limits),
            T0,
        )
        .unwrap();
        let snapshot = OperationSnapshot {
            operation,
            version: 1,
            steps: Vec::new(),
            edges: Vec::new(),
            executions: Vec::new(),
            incidents: Vec::new(),
        };
        Self {
            state: OperationState::new(snapshot),
            ids: CounterIds::new(),
            now: T0,
            subject,
        }
    }

    /// An admitted operation.
    pub fn running(limits: BudgetLimits) -> Self {
        let mut fx = Self::new(limits);
        fx.event(OperationEvent::Admit);
        fx
    }

    /// Decides `event` and applies the decision, or returns why it was refused. A refusal
    /// leaves the state as it was.
    pub fn try_event(&mut self, event: OperationEvent) -> Result<Decision, DecideError> {
        let decision = decide(&self.state, event, self.now, &mut self.ids)?;
        self.state = self.state.apply(&decision);
        Ok(decision)
    }

    pub fn event(&mut self, event: OperationEvent) -> Decision {
        self.try_event(event).unwrap()
    }

    pub fn advance(&mut self, seconds: i64) {
        self.now += SignedDuration::from_secs(seconds);
    }

    pub fn status(&self) -> OperationStatus {
        self.state.operation().status.clone()
    }

    pub fn outcome(&self, step: StepId) -> StepOutcome {
        self.state.step(step).unwrap().progress.outcome
    }

    /// Changes the stored snapshot the way a writer changes rows before it decides.
    pub fn tweak(&mut self, change: impl FnOnce(&mut OperationSnapshot)) {
        let mut snapshot = self.state.snapshot().clone();
        change(&mut snapshot);
        self.state = OperationState::new(snapshot);
    }

    pub fn new_step(&mut self, intent: StepIntent) -> NewStep {
        NewStep {
            id: self.ids.step(),
            kind: intent.kind(),
            phase: "work".to_owned(),
            parent: None,
            intent,
        }
    }

    pub fn note(&mut self) -> NewStep {
        self.new_step(StepIntent::Note {
            text: "noted".to_owned(),
            author: Actor::System("operations".to_owned()),
        })
    }

    pub fn plan(&mut self) -> NewStep {
        self.new_step(StepIntent::Plan {
            text: "restart it".to_owned(),
        })
    }

    pub fn evidence(&mut self) -> NewStep {
        let host = self.subject.host;
        self.new_step(StepIntent::EvidenceRequest {
            host,
            tool: "docker_ps".to_owned(),
            args: serde_json::json!({}),
        })
    }

    pub fn question(&mut self, expires_after: i64) -> NewStep {
        let expires_at = self.now + SignedDuration::from_secs(expires_after);
        self.new_step(StepIntent::Question {
            text: "may I restart it?".to_owned(),
            expires_at,
        })
    }

    /// Proposes `steps` (no edges) and returns their ids.
    pub fn propose(&mut self, steps: Vec<NewStep>) -> Vec<StepId> {
        let ids = steps.iter().map(|s| s.id).collect();
        self.event(OperationEvent::Propose {
            steps,
            edges: Vec::new(),
        });
        ids
    }

    pub fn fresh_step(&mut self) -> StepId {
        self.ids.step()
    }

    pub fn propose_plan(&mut self) -> StepId {
        let step = self.plan();
        self.propose(vec![step])[0]
    }

    pub fn propose_evidence(&mut self) -> StepId {
        let step = self.evidence();
        self.propose(vec![step])[0]
    }

    pub fn propose_question(&mut self, expires_after: i64) -> StepId {
        let step = self.question(expires_after);
        self.propose(vec![step])[0]
    }

    /// Proposes a note then a plan and returns the plan's id.
    pub fn propose_two(&mut self) -> StepId {
        let (note, plan) = (self.note(), self.plan());
        self.propose(vec![note, plan])[1]
    }

    /// The intent of a restart of the pinned container with `args`.
    pub fn intent(&self, args: &[(&str, &str)]) -> ExecutionIntent {
        ExecutionIntent {
            contract: ContractId::parse("skill.restart_container").unwrap(),
            contract_version: 1,
            host: self.subject.host,
            check: self.subject.check.clone(),
            target: self.subject.target.clone(),
            args: args
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
            retry_of: None,
        }
    }

    /// A proposal to run `intent`, approvable for a day.
    pub fn proposal(&mut self, intent: ExecutionIntent) -> ExecutionProposal {
        ExecutionProposal {
            intent,
            step: self.ids.step(),
            approval_step: self.ids.step(),
            parent: None,
            caused_by: None,
            plan_hash: Some(hash(0xab)),
            risk: domovoi_core::Risk::Write,
            expires_at: self.now + SignedDuration::from_hours(24),
            refused: None,
        }
    }

    /// Proposes the default run and returns its Execution step.
    pub fn propose_run(&mut self) -> StepId {
        let intent = self.intent(&[]);
        self.propose_intent(intent)
    }

    pub fn propose_intent(&mut self, intent: ExecutionIntent) -> StepId {
        let proposal = self.proposal(intent);
        let step = proposal.step;
        self.event(OperationEvent::ProposeExecution(proposal));
        step
    }

    /// Gives the stored row a person's approval, as the writer does before it decides.
    pub fn approve_row(&mut self, step: StepId) {
        let now = self.now;
        self.tweak(|snapshot| {
            let row = snapshot
                .executions
                .iter_mut()
                .find(|row| row.value.step == step)
                .unwrap();
            let binding = PlanBinding::Full(row.value.plan_hash);
            row.value
                .authorize(
                    approvals::authorize(approvals::human(UserId::new())),
                    approvals::provenance(),
                    &binding,
                    now,
                )
                .unwrap();
        });
    }

    /// Approves the run and decides the authorization.
    pub fn authorize_run(&mut self, step: StepId) -> Decision {
        self.approve_row(step);
        self.event(OperationEvent::ExecutionAuthorized { step })
    }

    /// The agent is away: decides the event, then stamps `waiting_since` on the row as the
    /// writer does.
    pub fn wait_for_agent(&mut self, step: StepId) {
        self.event(OperationEvent::WaitingForAgent { step });
        let now = self.now;
        self.tweak(|snapshot| {
            let row = snapshot
                .executions
                .iter_mut()
                .find(|row| row.value.step == step)
                .unwrap();
            row.value.waiting_since.get_or_insert(now);
        });
    }

    pub fn dispatch_run(&mut self, step: StepId) -> Decision {
        let epoch = JournalEpoch::from_uuid(Uuid::from_u128(7));
        let reconcile_after = self.now + SignedDuration::from_secs(60);
        self.event(OperationEvent::ExecutionDispatched {
            step,
            epoch,
            reconcile_after,
        })
    }

    pub fn settle(
        &mut self,
        step: StepId,
        outcome: StepOutcome,
        verify_after: Option<SignedDuration>,
    ) -> Result<Decision, DecideError> {
        self.try_event(OperationEvent::ExecutionSettled {
            step,
            outcome,
            reason: None,
            output: None,
            verify_after,
        })
    }

    /// A run proposed, approved and dispatched; its step.
    pub fn dispatched_run(&mut self) -> StepId {
        let step = self.propose_run();
        self.authorize_run(step);
        self.dispatch_run(step);
        step
    }

    /// The Verification step that checks `execution`.
    pub fn verification_of(&self, execution: StepId) -> StepId {
        self.state
            .snapshot()
            .steps
            .iter()
            .find(|r| {
                matches!(&r.step.intent, StepIntent::Verification { execution: e, .. } if *e == execution)
            })
            .unwrap()
            .step
            .id
    }
}
