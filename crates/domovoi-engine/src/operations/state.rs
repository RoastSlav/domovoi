//! The operation as `decide` sees it: one consistent snapshot and its lookups.

use domovoi_core::ports::Versioned;
use domovoi_core::{
    ExecutionRecord, Operation, OperationSnapshot, StepId, StepOutcome, StepRecord,
};

use super::decide::Decision;

/// An [`OperationSnapshot`] with the lookups `decide` needs. Built from one read of the
/// store; `decide` never changes it.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationState {
    snapshot: OperationSnapshot,
}

impl OperationState {
    /// Wraps a snapshot.
    #[must_use]
    pub fn new(snapshot: OperationSnapshot) -> Self {
        Self { snapshot }
    }

    /// The snapshot.
    #[must_use]
    pub fn snapshot(&self) -> &OperationSnapshot {
        &self.snapshot
    }

    /// Gives the snapshot back.
    #[must_use]
    pub fn into_snapshot(self) -> OperationSnapshot {
        self.snapshot
    }

    /// The operation.
    #[must_use]
    pub fn operation(&self) -> &Operation {
        &self.snapshot.operation
    }

    /// The step with `id`.
    #[must_use]
    pub fn step(&self, id: StepId) -> Option<&StepRecord> {
        self.snapshot.step(id)
    }

    /// The execution row of Execution step `step`.
    #[must_use]
    pub fn execution_of(&self, step: StepId) -> Option<&ExecutionRecord> {
        self.snapshot.execution_of(step).map(|row| &row.value)
    }

    /// The steps that are pending or running, in ordinal order.
    pub fn open_steps(&self) -> impl Iterator<Item = &StepRecord> {
        self.snapshot.open_steps()
    }

    /// How many steps the operation has, which is the next ordinal.
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.snapshot.steps.len()
    }

    /// The state after `decision` is committed: the operation, new steps, edges and
    /// execution rows added, and progress changes applied. It does not apply the execution
    /// row changes that belong to the writer.
    #[must_use]
    pub fn apply(&self, decision: &Decision) -> Self {
        let mut next = self.snapshot.clone();
        next.operation = decision.operation.clone();
        next.steps.extend(decision.steps.iter().cloned());
        next.edges.extend(decision.edges.iter().copied());
        next.executions
            .extend(decision.executions.iter().map(|row| Versioned {
                value: row.clone(),
                version: 1,
            }));
        for change in &decision.progress {
            if let Some(record) = next.steps.iter_mut().find(|r| r.step.id == change.step) {
                debug_assert!(record.progress.outcome == change.from);
                record.progress = change.to.clone();
            }
        }
        next.version += 1;
        Self { snapshot: next }
    }

    /// Whether any step is still pending or running.
    #[must_use]
    pub fn has_open_steps(&self) -> bool {
        self.snapshot.steps.iter().any(|r| {
            matches!(
                r.progress.outcome,
                StepOutcome::Pending | StepOutcome::Running
            )
        })
    }
}
