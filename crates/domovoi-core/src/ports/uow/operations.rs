//! The state writes of the operation aggregate, applied inside a [`Transaction`](super::Transaction).

use crate::id::{IncidentId, OperationId, StepId};
use crate::operation::{ExecutionRecord, Operation, Step, StepEdge, StepOutcome, StepProgress};

use super::Expected;

/// What an execution write expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionGuard {
    /// No execution row exists for the step yet.
    New,
    /// The stored row is at this version.
    Version(u64),
}

/// One typed change to an operation and what it owns.
///
/// Built only by the operation writer in `domovoi-engine`; the unit of work applies whatever
/// it is given and enforces the structural rules (dense ordinals, edges pointing backwards,
/// one execution row per Execution step).
#[derive(Debug, Clone)]
pub enum OperationWrite {
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`) the operation row. The
    /// store bumps the version.
    Operation {
        /// The operation to store.
        operation: Operation,
        /// What the stored row must look like.
        expected: Expected,
    },
    /// Inserts a step. Steps are never updated: progress is a separate write.
    Step(Step),
    /// Moves a step's progress on, only while its stored outcome is `from`.
    Progress {
        /// The step.
        step: StepId,
        /// The outcome the caller read.
        from: StepOutcome,
        /// The new progress.
        to: StepProgress,
    },
    /// Inserts an edge. The store checks the ordinal rule.
    Edge(StepEdge),
    /// Inserts (`ExecutionGuard::New`) or replaces an execution row at the guarded version.
    Execution {
        /// The row to store.
        execution: ExecutionRecord,
        /// What the stored row must look like.
        expected: ExecutionGuard,
    },
    /// Links an incident to an operation. Idempotent.
    IncidentLink {
        /// The operation.
        operation: OperationId,
        /// The incident.
        incident: IncidentId,
    },
}
