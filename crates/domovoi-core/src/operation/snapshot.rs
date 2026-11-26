//! One consistent read of an operation with everything it owns.

use crate::id::{ExecutionId, IncidentId, StepId};
use crate::ports::uow::Versioned;

use super::aggregate::Operation;
use super::edge::{EdgeKind, StepEdge};
use super::execution::ExecutionRecord;
use super::status::WaitKind;
use super::step::{StepKind, StepOutcome, StepRecord};

/// An operation, its steps in ordinal order, its edges, its execution rows and its linked
/// incidents, all read together.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationSnapshot {
    /// The operation.
    pub operation: Operation,
    /// The operation row's version, which a later write must name.
    pub version: u64,
    /// The steps, ordered by ordinal.
    pub steps: Vec<StepRecord>,
    /// The edges.
    pub edges: Vec<StepEdge>,
    /// The execution rows with their versions.
    pub executions: Vec<Versioned<ExecutionRecord>>,
    /// The incidents the operation is about.
    pub incidents: Vec<IncidentId>,
}

impl OperationSnapshot {
    /// The step with `id`.
    #[must_use]
    pub fn step(&self, id: StepId) -> Option<&StepRecord> {
        self.steps.iter().find(|record| record.step.id == id)
    }

    /// The execution row of Execution step `step`.
    #[must_use]
    pub fn execution_of(&self, step: StepId) -> Option<&Versioned<ExecutionRecord>> {
        let execution = ExecutionId::from_step(step);
        self.executions
            .iter()
            .find(|row| row.value.execution == execution)
    }

    /// Whether the reasoning turn `turn` was proposed after a turn that failed or was skipped,
    /// which is how a retry waits for its due time.
    fn is_parked_retry(&self, turn: StepId) -> bool {
        self.edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::CausedBy && edge.from == turn)
            .filter_map(|edge| self.steps.iter().find(|record| record.step.id == edge.to))
            .any(|cause| {
                cause.step.kind == StepKind::ReasoningTurn
                    && matches!(
                        cause.progress.outcome,
                        StepOutcome::Failed | StepOutcome::NotRun
                    )
            })
    }

    /// Steps that are pending or running, in ordinal order.
    pub fn open_steps(&self) -> impl Iterator<Item = &StepRecord> {
        self.steps
            .iter()
            .filter(|record| !record.progress.outcome.is_final())
    }

    /// What the operation is waiting for, from its steps alone, in priority order: a pending
    /// Approval, a pending or started Question, a pending Execution that was approved and is waiting for
    /// its agent, a pending Verification.
    #[must_use]
    pub fn waits(&self) -> Vec<WaitKind> {
        let mut waits = Vec::new();
        for record in &self.steps {
            let outcome = record.progress.outcome;
            let pending = outcome == StepOutcome::Pending;
            let wait = match record.step.kind {
                // A question that was started is still waiting for its answer.
                StepKind::Question if !outcome.is_final() => WaitKind::User,
                // A turn that follows a failed or skipped one is a retry the operation parked:
                // it is proposed at once and does its work when its due time comes.
                StepKind::ReasoningTurn
                    if !outcome.is_final() && self.is_parked_retry(record.step.id) =>
                {
                    WaitKind::Timer
                }
                _ if !pending => continue,
                StepKind::Approval => WaitKind::Approval,
                StepKind::Execution => {
                    let waiting = self.execution_of(record.step.id).is_some_and(|row| {
                        row.value.approval().is_some() && row.value.waiting_since.is_some()
                    });
                    if !waiting {
                        continue;
                    }
                    WaitKind::Agent
                }
                StepKind::Verification => WaitKind::Timer,
                _ => continue,
            };
            if !waits.contains(&wait) {
                waits.push(wait);
            }
        }
        waits.sort();
        waits
    }
}
