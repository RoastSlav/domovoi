//! One consistent read of an operation with everything it owns.

use crate::id::{ExecutionId, IncidentId, StepId};
use crate::ports::uow::Versioned;

use super::aggregate::Operation;
use super::edge::StepEdge;
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
