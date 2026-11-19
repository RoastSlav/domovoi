//! The public face of the operation service: every command a caller may give.
//!
//! A handle is cheap to clone. Each method queues one command for the writer task and waits
//! for its answer; the writer refuses what the decision function refuses and commits the
//! rest in one transaction.

use std::sync::Arc;

use domovoi_core::ports::{EffectKind, HostReconciler};
use domovoi_core::{
    Actor, ApprovalProvenance, Authorization, BudgetKind, BudgetLimits, ExecutionIntent,
    FinishOutcome, HostId, HumanApproval, IncidentId, OperationId, OperationKind, PinnedSubject,
    PlanBinding, StepId, StepIntent, StepKind, StepOutcome, StepUsage,
};
use serde_json::Value;
use tokio::sync::oneshot;

use super::decide::{NewEdge, NewStep};
use super::error::OperationError;
use super::proposal;
use super::writer::{OpShared, OperationCommand, Reply};

/// What a new operation starts from.
#[derive(Debug, Clone)]
pub struct OperationSeed {
    /// What kind of operation it is.
    pub kind: OperationKind,
    /// What it is trying to achieve, 1 to 500 characters, already redacted.
    pub goal: String,
    /// What it may act on, 1 to 8 subjects.
    pub pinned: Vec<PinnedSubject>,
    /// The incidents it is about.
    pub incidents: Vec<IncidentId>,
    /// The operation it was started from, if any.
    pub parent: Option<OperationId>,
    /// Its limits; the service defaults apply when absent.
    pub budgets: Option<BudgetLimits>,
    /// Steps it starts with, under the rules of [`OperationHandle::propose`].
    pub first_steps: Vec<NewStep>,
    /// Effects to create with it, addressed to the new operation.
    pub effects: Vec<(EffectKind, Value)>,
    /// Who created it.
    pub actor: Actor,
}

/// A run of a capability that a caller wants proposed.
#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    /// What to run, where and with which arguments.
    pub intent: ExecutionIntent,
    /// The step the new steps follow from, if any.
    pub parent: Option<StepId>,
    /// The step that led to this proposal, if any.
    pub caused_by: Option<StepId>,
}

/// How a step ended.
#[derive(Debug, Clone)]
pub struct StepFinish {
    /// The final outcome.
    pub outcome: StepOutcome,
    /// A one-line summary.
    pub summary: Option<String>,
    /// Why it ended as it did.
    pub reason: Option<String>,
    /// What it produced; stored on the step only.
    pub output: Option<String>,
    /// What a model turn used.
    pub usage: Option<StepUsage>,
}

/// Reaches the operation service. Cheap to clone.
#[derive(Clone)]
pub struct OperationHandle {
    pub(super) shared: Arc<OpShared>,
}

impl std::fmt::Debug for OperationHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationHandle")
            .field("settings", &self.shared.settings)
            .finish_non_exhaustive()
    }
}

impl OperationHandle {
    /// Queues the command `make` builds and waits for its answer.
    async fn ask<T>(
        &self,
        make: impl FnOnce(Reply<T>) -> OperationCommand,
    ) -> Result<T, OperationError> {
        let (reply, answer) = oneshot::channel();
        if self.shared.writer.send(make(reply)).await.is_err() {
            return Err(OperationError::Unavailable);
        }
        answer.await.map_err(|_| OperationError::Unavailable)?
    }

    /// Whether the writer is still taking commands.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.shared.writer.is_closed()
    }

    /// Creates an operation from `seed`. It starts queued and is admitted when a slot is free.
    pub async fn create(&self, seed: OperationSeed) -> Result<OperationId, OperationError> {
        self.ask(|reply| OperationCommand::Create {
            seed: Box::new(seed),
            reply,
        })
        .await
    }

    /// Adds steps the writer does not own (never an Execution, Approval, Verification or
    /// PolicyDecision) and edges between steps. Returns the new steps' ids.
    pub async fn propose(
        &self,
        operation: OperationId,
        steps: Vec<NewStep>,
        edges: Vec<NewEdge>,
    ) -> Result<Vec<StepId>, OperationError> {
        let ids = steps.iter().map(|step| step.id).collect();
        self.ask(|reply| OperationCommand::Propose {
            operation,
            steps,
            edges,
            actor: system(),
            reply,
        })
        .await?;
        Ok(ids)
    }

    /// Proposes a run of a capability: an Approval step and an Execution step that waits for
    /// a person. Returns the Execution step's id, also when the step was stored refused (an
    /// unknown skill, bad arguments, an unpinned target). A host with no live agent session
    /// is an error and nothing is stored.
    pub async fn propose_execution(
        &self,
        operation: OperationId,
        request: ExecutionRequest,
    ) -> Result<StepId, OperationError> {
        let (step, proposal) = proposal::propose(&self.shared, request).await?;
        self.ask(|reply| OperationCommand::ProposeExecution {
            operation,
            proposal: Box::new(proposal),
            actor: system(),
            reply,
        })
        .await?;
        Ok(step)
    }

    /// Records a person's approval of an execution. The binding must name the stored plan
    /// hash; a mismatch, an expired window or a second approval changes nothing. A policy
    /// authorization is refused.
    pub async fn authorize(
        &self,
        operation: OperationId,
        step: StepId,
        authorization: Authorization,
        binding: PlanBinding,
        provenance: ApprovalProvenance,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::Authorize {
            operation,
            step,
            authorization,
            binding,
            provenance,
            reply,
        })
        .await
    }

    /// Records that the person in `approval` denied an execution.
    pub async fn deny(
        &self,
        operation: OperationId,
        step: StepId,
        approval: HumanApproval,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::Deny {
            operation,
            step,
            approval,
            reply,
        })
        .await
    }

    /// Records `actor`'s answer to a pending question as a note. Returns the note's id.
    pub async fn answer(
        &self,
        operation: OperationId,
        question: StepId,
        text: String,
        actor: Actor,
    ) -> Result<StepId, OperationError> {
        let note = NewStep {
            id: StepId::new(),
            kind: StepKind::Note,
            phase: "answer".to_owned(),
            parent: Some(question),
            intent: StepIntent::Note {
                text,
                author: actor.clone(),
            },
        };
        let id = note.id;
        self.ask(|reply| OperationCommand::Answer {
            operation,
            question,
            note,
            actor,
            reply,
        })
        .await?;
        Ok(id)
    }

    /// Marks a pending step as started.
    pub async fn step_started(
        &self,
        operation: OperationId,
        step: StepId,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::StepStarted {
            operation,
            step,
            actor: system(),
            reply,
        })
        .await
    }

    /// Records how a step ended. Execution, Approval and Verification steps end through
    /// their own commands and are refused here.
    pub async fn step_finished(
        &self,
        operation: OperationId,
        step: StepId,
        finish: StepFinish,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::StepFinished {
            operation,
            step,
            outcome: finish.outcome,
            summary: finish.summary,
            reason: finish.reason,
            output: finish.output,
            usage: finish.usage,
            actor: system(),
            reply,
        })
        .await
    }

    /// Cancels an operation. A dispatched execution is not recalled; its result is recorded
    /// when it arrives.
    pub async fn cancel(
        &self,
        operation: OperationId,
        actor: Actor,
        reason: String,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::Cancel {
            operation,
            actor,
            reason,
            reply,
        })
        .await
    }

    /// Pauses an operation until `actor` or another person resumes it.
    pub async fn pause(&self, operation: OperationId, actor: Actor) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::Pause {
            operation,
            actor,
            reply,
        })
        .await
    }

    /// Resumes a paused operation after raising the named limits (`None` removes a limit). An
    /// operation paused for a spent budget needs that budget to have room again.
    pub async fn resume(
        &self,
        operation: OperationId,
        actor: Actor,
        extend: Vec<(BudgetKind, Option<u64>)>,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::Resume {
            operation,
            actor,
            extend,
            reply,
        })
        .await
    }

    /// Ends an operation. Refused while an execution is running.
    pub async fn finish(
        &self,
        operation: OperationId,
        outcome: FinishOutcome,
        summary: String,
    ) -> Result<(), OperationError> {
        self.ask(|reply| OperationCommand::Finish {
            operation,
            outcome,
            summary,
            reply,
        })
        .await
    }
}

/// The actor for what the service's callers do on the service's behalf.
fn system() -> Actor {
    Actor::System(domovoi_core::SYSTEM_OPERATIONS.to_owned())
}

#[async_trait::async_trait]
impl HostReconciler for OperationHandle {
    async fn reconcile_host(&self, host: HostId) {
        let (reply, answer) = oneshot::channel();
        if self
            .shared
            .writer
            .send(OperationCommand::ReconcileHost { host, reply })
            .await
            .is_ok()
        {
            let _ = answer.await;
        }
    }
}
