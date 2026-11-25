//! The commands the writer task applies. Public commands come from [`OperationHandle`]
//! methods; the handler commands are crate-private, built only by the effect handlers, and
//! each carries the effect it answers so the answer and the effect's completion commit
//! together.
//!
//! [`OperationHandle`]: crate::operations::OperationHandle

use domovoi_core::ports::ClaimToken;
use domovoi_core::{
    Actor, AgentSessionId, ApprovalProvenance, Authorization, BudgetKind, EffectId, FinishOutcome,
    HostId, HumanApproval, JournalEpoch, OperationId, PlanBinding, PlanHash, StepId, StepOutcome,
    StepUsage,
};
use jiff::SignedDuration;
use tokio::sync::oneshot;

use crate::operations::decide::{EffectSpec, ExecutionProposal, NewEdge, NewStep, RecordedStep};
use crate::operations::error::OperationError;
use crate::operations::handle::OperationSeed;
use crate::operations::resolution::{
    EditSummaryError, EditSummaryField, EditedField, PlannedRevision,
};

/// Where a command's answer goes.
pub(crate) type Reply<T> = oneshot::Sender<Result<T, OperationError>>;

/// The effect a handler command answers: completed in the same commit as the change.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    /// The effect.
    pub(crate) effect: EffectId,
    /// The claim that fences it; a stale claim changes nothing.
    pub(crate) claim: ClaimToken,
}

/// How a step ends in a [`OperationCommand::Conclude`].
#[derive(Debug, Clone)]
pub(crate) struct Conclusion {
    /// The step.
    pub(crate) step: StepId,
    /// The final outcome.
    pub(crate) outcome: StepOutcome,
    /// A one-line summary.
    pub(crate) summary: Option<String>,
    /// Why it ended so.
    pub(crate) reason: Option<String>,
    /// What it produced; stored on the step only.
    pub(crate) output: Option<String>,
    /// What a model turn used.
    pub(crate) usage: Option<StepUsage>,
}

/// How a model turn ends, with the reads it made and the effects that follow.
#[derive(Debug, Clone)]
pub(crate) struct TurnEnd {
    /// The `CallModel` effect the turn answers.
    pub(crate) held: Held,
    /// The operation.
    pub(crate) operation: OperationId,
    /// How the turn ends.
    pub(crate) turn: Conclusion,
    /// The reads the turn made, recorded as evidence steps.
    pub(crate) evidence: Vec<RecordedStep>,
    /// Effects to create in the same commit.
    pub(crate) successors: Vec<EffectSpec>,
}

/// What the writer decided about a dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchAnswer {
    /// The execution is now dispatched under this journal epoch.
    Dispatched(JournalEpoch),
    /// The writer settled the execution instead, with this outcome.
    Settled(StepOutcome),
}

/// One change for the writer task.
pub(crate) enum OperationCommand {
    /// Creates an operation, then admits queued ones.
    Create {
        seed: Box<OperationSeed>,
        reply: Reply<OperationId>,
    },
    /// Adds steps and edges.
    Propose {
        operation: OperationId,
        steps: Vec<NewStep>,
        edges: Vec<NewEdge>,
        actor: Actor,
        reply: Reply<()>,
    },
    /// Adds an Approval and an Execution step, or a refused Execution step.
    ProposeExecution {
        operation: OperationId,
        proposal: Box<ExecutionProposal>,
        actor: Actor,
        reply: Reply<()>,
    },
    /// A person approved an execution.
    Authorize {
        operation: OperationId,
        step: StepId,
        authorization: Authorization,
        binding: PlanBinding,
        provenance: ApprovalProvenance,
        chat_token: Option<String>,
        reply: Reply<()>,
    },
    /// The permit issuer authorized an execution from an enforced policy `Allow`. The engine
    /// only carries the authorization it was handed; it never builds one.
    AuthorizeByPolicy {
        held: Held,
        operation: OperationId,
        step: StepId,
        authorization: Authorization,
        /// The enforced decision document, stored with the permit.
        decision: String,
        reply: Reply<()>,
    },
    /// Replaces the chat token hash of a step still awaiting approval.
    RotateToken {
        operation: OperationId,
        step: StepId,
        hash: [u8; 32],
        reply: Reply<()>,
    },
    /// A person denied an execution.
    Deny {
        operation: OperationId,
        step: StepId,
        approval: HumanApproval,
        /// The chat token a chat denial must carry; the web passes `None`.
        chat_token: Option<String>,
        reply: Reply<()>,
    },
    /// A person answered a question.
    Answer {
        operation: OperationId,
        question: StepId,
        note: NewStep,
        actor: Actor,
        reply: Reply<()>,
    },
    /// A step started.
    StepStarted {
        operation: OperationId,
        step: StepId,
        actor: Actor,
        reply: Reply<()>,
    },
    /// A step ended.
    StepFinished {
        operation: OperationId,
        step: StepId,
        outcome: StepOutcome,
        summary: Option<String>,
        reason: Option<String>,
        output: Option<String>,
        usage: Option<StepUsage>,
        actor: Actor,
        reply: Reply<()>,
    },
    /// Stops an operation for good.
    Cancel {
        operation: OperationId,
        actor: Actor,
        reason: String,
        reply: Reply<()>,
    },
    /// Stops an operation until a person resumes it.
    Pause {
        operation: OperationId,
        actor: Actor,
        reply: Reply<()>,
    },
    /// Resumes a paused operation.
    Resume {
        operation: OperationId,
        actor: Actor,
        extend: Vec<(BudgetKind, Option<u64>)>,
        reply: Reply<()>,
    },
    /// Ends an operation.
    Finish {
        operation: OperationId,
        outcome: FinishOutcome,
        summary: String,
        reply: Reply<()>,
    },
    /// Queues a reconcile for each execution dispatched to a host.
    ReconcileHost {
        host: HostId,
        reply: oneshot::Sender<()>,
    },
    /// Handler: a pending step started.
    StartStep {
        held: Held,
        operation: OperationId,
        step: StepId,
        reply: Reply<()>,
    },
    /// Handler: a step ended.
    FinishStep {
        held: Held,
        operation: OperationId,
        step: StepId,
        outcome: StepOutcome,
        summary: Option<String>,
        reason: Option<String>,
        output: Option<String>,
        reply: Reply<()>,
    },
    /// Handler: finish the held effect and create its successor in one commit. `waiting`
    /// also records that the operation now waits for the step's agent.
    Defer {
        held: Held,
        operation: OperationId,
        step: Option<StepId>,
        successor: Box<EffectSpec>,
        waiting: bool,
        reply: Reply<()>,
    },
    /// Handler: check an authorized execution one last time and record the dispatch, or
    /// settle it.
    Dispatch {
        held: Held,
        operation: OperationId,
        step: StepId,
        plan: PlanHash,
        session: AgentSessionId,
        epoch: JournalEpoch,
        reply: Reply<DispatchAnswer>,
    },
    /// Handler: an execution ended.
    Settle {
        held: Option<Held>,
        operation: OperationId,
        step: StepId,
        outcome: StepOutcome,
        reason: Option<String>,
        output: Option<String>,
        verify_after: Option<SignedDuration>,
        reply: Reply<()>,
    },
    /// Handler: a verification finished.
    Verified {
        held: Held,
        operation: OperationId,
        step: StepId,
        passed: bool,
        summary: String,
        reply: Reply<()>,
    },
    /// Handler: an approval window closed.
    Expire {
        held: Held,
        operation: OperationId,
        step: StepId,
        reply: Reply<()>,
    },
    /// Handler: measure wall-clock time.
    Tick {
        held: Held,
        operation: OperationId,
        reply: Reply<()>,
    },
    /// Handler: the held effect is done. Optionally ends a step and creates effects, all in
    /// the same commit.
    Conclude {
        held: Held,
        operation: OperationId,
        step: Option<Conclusion>,
        successors: Vec<EffectSpec>,
        reply: Reply<()>,
    },
    /// Handler: a model turn ended. Its reads become evidence steps, the turn ends, the
    /// successors are created and the held effect completes, all in one commit.
    FinishTurn { end: Box<TurnEnd>, reply: Reply<()> },
    /// Handler: the held effect has nothing left to do.
    FinishEffect { held: Held, reply: Reply<()> },
    /// Handler: append the planned summary revisions and complete the held effect.
    WriteResolution {
        operation: OperationId,
        revisions: Vec<PlannedRevision>,
        held: Held,
        reply: Reply<()>,
    },
    /// An owner's edit of a narrative field of a finished operation's summary.
    EditSummary {
        operation: OperationId,
        edit: EditSummaryField,
        actor: Actor,
        reply: oneshot::Sender<Result<EditedField, EditSummaryError>>,
    },
}
