//! The operation aggregate: a durable record of one piece of work on hosts, made of immutable
//! steps joined by typed edges (ARCHITECTURE.md §5).
//!
//! Pure types and rules only. Writing them is the operation writer's job in `domovoi-engine`;
//! persisting them is the unit of work's.

mod aggregate;
mod budget;
mod edge;
mod execution;
mod intent;
mod snapshot;
mod status;
mod step;
mod trace;

pub use aggregate::{MAX_GOAL_CHARS, MAX_PINNED, Operation, PinnedSubject};
pub use budget::{BudgetKind, BudgetLimits, BudgetUsed, Budgets};
pub use edge::{EdgeError, EdgeKind, StepEdge, check_edge};
pub use execution::{
    ExecutionApproval, ExecutionError, ExecutionParts, ExecutionRecord, ExecutionState,
};
pub use intent::{ExecutionIntent, RetryOf, StepIntent};
pub use snapshot::OperationSnapshot;
pub use status::{FinishOutcome, OperationKind, OperationStatus, PauseReason, WaitKind};
pub use step::{
    MAX_OUTPUT_BYTES, MAX_SUMMARY_CHARS, Step, StepKind, StepOutcome, StepProgress, StepRecord,
    StepUsage, cut_chars,
};
pub use trace::{
    OperationTrace, TRACE_SCHEMA_VERSION, TraceEdge, TraceOperation, TraceRef, TraceStep,
    build_trace,
};
