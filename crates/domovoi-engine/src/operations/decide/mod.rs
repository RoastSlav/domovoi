//! The pure decision function of the operation aggregate (ARCHITECTURE.md §5).
//!
//! [`decide`] takes the operation as it is stored and one [`OperationEvent`] and returns
//! everything that should change in one commit, or says why nothing may. It does no I/O,
//! reads no clock (the caller passes `now`), draws no randomness (the caller passes ids) and
//! never changes its input: it works on a clone.
//!
//! The writer does a few row changes itself before it calls `decide`, because the rows are
//! not the decision's to make: it applies [`domovoi_core::ExecutionRecord::authorize`] before
//! [`OperationEvent::ExecutionAuthorized`], and it stamps `waiting_since`, `dispatched` and
//! `settled_at` on the execution row. `decide` reads the authorized row; for
//! [`OperationEvent::WaitingForAgent`] it assumes the stamp when it derives the status.

mod effects;
mod events;
mod execution;
mod finish;
mod permit;
mod resolution;
mod status;
mod steps;
mod turn;
mod verify;
mod work;

use domovoi_core::ports::{EffectKind, JournalKind};
use domovoi_core::{
    BudgetKind, EffectId, ExecutionRecord, Operation, StepEdge, StepId, StepKind, StepOutcome,
    StepProgress, StepRecord,
};
use jiff::Timestamp;
use serde_json::Value;

pub(crate) use effects::{
    DISPATCH_ATTEMPTS, RECONCILE_ATTEMPTS, WAKE_ATTEMPTS, dispatch_key, reconcile_key,
    step_payload, wake_payload,
};
pub use events::{ExecutionProposal, NewEdge, NewStep, OperationEvent, RecordedStep};
pub(crate) use resolution::{resolution_key, resolution_payload};
pub use status::derive_status;

use super::state::OperationState;
use work::Work;

/// Where `decide` gets fresh ids, so it stays free of randomness.
pub trait IdSource {
    /// A new step id.
    fn step(&mut self) -> StepId;
    /// A new effect id.
    fn effect(&mut self) -> EffectId;
}

/// An effect to create in the same commit.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSpec {
    /// The effect's id.
    pub id: EffectId,
    /// What kind of work it is.
    pub kind: EffectKind,
    /// Its input: JSON with `"v": 1` naming the operation and, where it applies, the step.
    pub payload: Value,
    /// When it may first be claimed.
    pub not_before: Timestamp,
    /// Attempts allowed before it is dead-lettered.
    pub max_attempts: u32,
    /// While an effect with this key is pending or claimed, a second one is refused.
    pub dedup_key: Option<String>,
}

/// A journal entry to append, without actor or time: the writer adds those.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalNote {
    /// The kind of entry.
    pub kind: JournalKind,
    /// The payload, version 1. Never holds a step's output.
    pub payload: Value,
}

/// An audit entry to append, without actor or time: the writer adds those. The detail always
/// names `operation` and `step`.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditNote {
    /// The kind, one of the constants in [`super::audit`].
    pub kind: &'static str,
    /// `operation:<id>` or `execution:<id>`.
    pub subject: String,
    /// The detail. Never holds output, a token or a full plan hash.
    pub detail: Value,
}

/// A change to the progress of a step that already exists.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressChange {
    /// The step.
    pub step: StepId,
    /// The outcome it is changing from; the store guards the write with it.
    pub from: StepOutcome,
    /// What it becomes.
    pub to: StepProgress,
}

/// Everything one event changes, to be committed together.
///
/// A new step carries its born progress in `steps` (an Assessment is born `succeeded`, a
/// Question `pending`), so a writer inserts the step and, when the born outcome is not
/// `pending`, writes the progress in the same commit. `progress` lists changes to steps that
/// already existed and to nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// The operation as it should be stored; `updated_at` is always `now`.
    pub operation: Operation,
    /// New steps, with ordinals continuing the existing count.
    pub steps: Vec<StepRecord>,
    /// New edges.
    pub edges: Vec<StepEdge>,
    /// New execution rows only. Changes to existing rows are the writer's.
    pub executions: Vec<ExecutionRecord>,
    /// Progress changes of existing steps.
    pub progress: Vec<ProgressChange>,
    /// Effects to create.
    pub effects: Vec<EffectSpec>,
    /// Journal entries, one per change, in order.
    pub journal: Vec<JournalNote>,
    /// Audit entries.
    pub audit: Vec<AuditNote>,
}

impl Decision {
    /// Whether the event changed nothing but the operation's `updated_at`, so a writer may
    /// skip the commit.
    #[must_use]
    pub fn is_noop(&self, state: &OperationState) -> bool {
        self.steps.is_empty()
            && self.edges.is_empty()
            && self.executions.is_empty()
            && self.progress.is_empty()
            && self.effects.is_empty()
            && self.journal.is_empty()
            && self.audit.is_empty()
            && self.operation.status == state.operation().status
            && self.operation.cycle == state.operation().cycle
            && self.operation.budgets == state.operation().budgets
    }
}

/// Why an event was refused. Nothing changes when `decide` returns one.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecideError {
    /// The operation is over, or not in a state this event applies to.
    #[error("the operation is not open for this")]
    OperationClosed,
    /// The operation is paused.
    #[error("the operation is paused")]
    Paused,
    /// No step has this id.
    #[error("no step {0}")]
    UnknownStep(StepId),
    /// The writer owns steps of this kind; they cannot be proposed.
    #[error("a {} step cannot be proposed this way", .0.as_str())]
    KindNotProposable(StepKind),
    /// An edge breaks the edge rules.
    #[error("{0}")]
    BadEdge(String),
    /// The step is not in a state this event applies to.
    #[error("step {0} is not open for this")]
    StepNotOpen(StepId),
    /// A budget that stopped the operation still has no room.
    #[error("the {0:?} budget is still spent; raise it to resume")]
    BudgetStillSpent(BudgetKind),
    /// An execution is running, so the operation cannot end yet.
    #[error("execution {0} is still running")]
    ExecutionInFlight(StepId),
    /// The event is malformed.
    #[error("{0}")]
    Invalid(String),
}

/// Decides what `event` changes. Pure: the same inputs give the same decision.
pub fn decide(
    state: &OperationState,
    event: OperationEvent,
    now: Timestamp,
    ids: &mut dyn IdSource,
) -> Result<Decision, DecideError> {
    let late = resolution::late_step(state, &event);
    let mut work = Work::new(state, now, ids);
    match event {
        OperationEvent::Admit => finish::admit(&mut work)?,
        OperationEvent::Propose { steps, edges } => steps::propose(&mut work, steps, edges)?,
        OperationEvent::ProposeExecution(proposal) => {
            execution::propose_execution(&mut work, proposal)?;
        }
        OperationEvent::StepStarted { step } => steps::started(&mut work, step)?,
        OperationEvent::StepFinished {
            step,
            outcome,
            summary,
            reason,
            output,
            usage,
        } => steps::finished(&mut work, step, outcome, summary, reason, output, usage)?,
        OperationEvent::TurnFinished {
            step,
            outcome,
            summary,
            reason,
            output,
            usage,
            evidence,
        } => {
            let ended = turn::Ending {
                outcome,
                summary,
                reason,
                output,
                usage,
            };
            turn::turn_finished(&mut work, step, ended, evidence)?;
        }
        OperationEvent::ExecutionAuthorized { step } => execution::authorized(&mut work, step)?,
        OperationEvent::ExecutionAuthorizedByPolicy { step } => {
            permit::authorized_by_policy(&mut work, step)?;
        }
        OperationEvent::ExecutionDenied { step } => execution::denied(&mut work, step)?,
        OperationEvent::ExecutionExpired { step } => execution::expired(&mut work, step)?,
        OperationEvent::WaitingForAgent { step } => execution::waiting_for_agent(&mut work, step)?,
        OperationEvent::ExecutionDispatched {
            step,
            epoch,
            reconcile_after,
        } => execution::dispatched(&mut work, step, epoch, reconcile_after)?,
        OperationEvent::ExecutionSettled {
            step,
            outcome,
            reason,
            output,
            verify_after,
        } => execution::settled(&mut work, step, outcome, reason, output, verify_after)?,
        OperationEvent::Verified {
            step,
            passed,
            summary,
        } => verify::verified(&mut work, step, passed, &summary)?,
        OperationEvent::Answer { question, note } => steps::answer(&mut work, question, note)?,
        OperationEvent::Cancel { reason } => finish::cancel(&mut work, &reason)?,
        OperationEvent::Pause { reason } => finish::pause(&mut work, reason)?,
        OperationEvent::Resume { extend } => finish::resume(&mut work, &extend)?,
        OperationEvent::Finish { outcome, summary } => {
            finish::finish(&mut work, outcome, &summary)?;
        }
        OperationEvent::Tick => finish::tick(&mut work)?,
    }
    let mut decision = work.into_decision();
    resolution::add_resolution_effect(state, late, &mut decision, ids, now);
    Ok(decision)
}
