//! The events [`super::decide`] understands, and the proposals they carry.

use domovoi_core::{
    BudgetKind, ExecutionIntent, FinishOutcome, JournalEpoch, PauseReason, PlanHash, StepId,
    StepIntent, StepKind, StepOutcome, StepUsage,
};
use jiff::{SignedDuration, Timestamp};

/// A step the caller wants added. The caller chooses the id; `kind` must match `intent`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewStep {
    /// The step's id.
    pub id: StepId,
    /// The kind of step; must be the kind of `intent`.
    pub kind: StepKind,
    /// A short label of the phase of work, 1 to 40 of `[a-z0-9_-]`.
    pub phase: String,
    /// The step it follows from, if any: an existing step or an earlier one in the batch.
    pub parent: Option<StepId>,
    /// What the step is for.
    pub intent: StepIntent,
}

/// An edge the caller wants added, from a dependent step to an earlier prerequisite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewEdge {
    /// The dependent step.
    pub from: StepId,
    /// The prerequisite.
    pub to: StepId,
    /// What the edge says.
    pub kind: domovoi_core::EdgeKind,
}

/// A proposed run of a capability.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionProposal {
    /// What to run.
    pub intent: ExecutionIntent,
    /// The id of the Execution step, which is also the execution's id.
    pub step: StepId,
    /// The id of the Approval step created before it. Unused when the proposal is refused.
    pub approval_step: StepId,
    /// The step the Execution step follows from, if any.
    pub parent: Option<StepId>,
    /// The step that made the model propose this, if any.
    pub caused_by: Option<StepId>,
    /// The v2 plan hash a person will approve. `None` only when `refused` is set.
    pub plan_hash: Option<PlanHash>,
    /// When the approval window closes.
    pub expires_at: Timestamp,
    /// Why the proposal cannot be made (unknown skill, bad arguments, a contract that does
    /// not execute): the step is stored refused with this text and nothing else is created.
    pub refused: Option<String>,
}

/// One thing that happened to an operation.
#[derive(Debug, Clone, PartialEq)]
pub enum OperationEvent {
    /// Leaves the queue and starts.
    Admit,
    /// Adds steps the writer does not own, and edges between steps.
    Propose {
        /// The steps, in order.
        steps: Vec<NewStep>,
        /// The edges.
        edges: Vec<NewEdge>,
    },
    /// Proposes a run of a capability: an Approval step and an Execution step.
    ProposeExecution(ExecutionProposal),
    /// A pending step started.
    StepStarted {
        /// The step.
        step: StepId,
    },
    /// A step reached a final outcome.
    StepFinished {
        /// The step.
        step: StepId,
        /// How it ended; must be final.
        outcome: StepOutcome,
        /// A one-line summary, already redacted.
        summary: Option<String>,
        /// Why it ended as it did.
        reason: Option<String>,
        /// What it produced, already redacted. Stored on the step only.
        output: Option<String>,
        /// What a model turn used.
        usage: Option<StepUsage>,
    },
    /// A person approved an execution. The execution row already holds the approval.
    ExecutionAuthorized {
        /// The Execution step.
        step: StepId,
    },
    /// A person denied an execution.
    ExecutionDenied {
        /// The Execution step.
        step: StepId,
    },
    /// An approval window closed with no decision.
    ExecutionExpired {
        /// The Execution step.
        step: StepId,
    },
    /// The agent for an approved execution is not connected; the hub is waiting for it.
    WaitingForAgent {
        /// The Execution step.
        step: StepId,
    },
    /// The execution was handed to its agent.
    ExecutionDispatched {
        /// The Execution step.
        step: StepId,
        /// The journal epoch of the agent session it was handed to.
        epoch: JournalEpoch,
        /// When the first reconcile attempt becomes due.
        reconcile_after: Timestamp,
    },
    /// An execution reached a final outcome.
    ExecutionSettled {
        /// The Execution step.
        step: StepId,
        /// How it ended.
        outcome: StepOutcome,
        /// Why, in plain words.
        reason: Option<String>,
        /// What it produced, already redacted.
        output: Option<String>,
        /// After a success, how long to wait before verifying; `None` for no verification.
        verify_after: Option<SignedDuration>,
    },
    /// A verification finished.
    Verified {
        /// The Verification step.
        step: StepId,
        /// Whether the check passed.
        passed: bool,
        /// One line on what was seen, already redacted.
        summary: String,
    },
    /// A person answered a question.
    Answer {
        /// The Question step.
        question: StepId,
        /// The Note step that holds the answer.
        note: NewStep,
    },
    /// Stops the operation.
    Cancel {
        /// Why, in plain words.
        reason: String,
    },
    /// Stops the operation until a person resumes it.
    Pause {
        /// Why it stopped.
        reason: PauseReason,
    },
    /// Resumes a paused operation, first raising the named limits.
    Resume {
        /// New limits; `None` removes the limit.
        extend: Vec<(BudgetKind, Option<u64>)>,
    },
    /// Ends the operation.
    Finish {
        /// How it ended.
        outcome: FinishOutcome,
        /// A sentence on how it ended.
        summary: String,
    },
    /// Measures wall-clock time against the budget.
    Tick,
}
