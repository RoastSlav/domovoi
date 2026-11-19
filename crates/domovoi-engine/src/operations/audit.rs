//! Audit kinds the operation service writes. The subject is `operation:<id>` or
//! `execution:<id>`; the detail always names `operation` and `step`.

/// An operation was created.
pub const OPERATION_CREATED: &str = "operation.created";
/// An operation was cancelled.
pub const OPERATION_CANCELLED: &str = "operation.cancelled";
/// An operation was paused, by a person or by a spent budget or an unknown outcome.
pub const OPERATION_PAUSED: &str = "operation.paused";
/// A paused operation was resumed.
pub const OPERATION_RESUMED: &str = "operation.resumed";
/// An execution was proposed and now waits for a person.
pub const EXECUTION_PROPOSED: &str = "execution.proposed";
/// A person approved an execution.
pub const EXECUTION_APPROVED: &str = "execution.approved";
/// A person denied an execution.
pub const EXECUTION_DENIED: &str = "execution.denied";
/// An approval window closed before anyone decided.
pub const EXECUTION_EXPIRED: &str = "execution.expired";
/// The loop guard stopped a dispatch.
pub const EXECUTION_BLOCKED: &str = "execution.blocked";
/// An approved execution was handed to its agent.
pub const EXECUTION_DISPATCHED: &str = "execution.dispatched";
/// An execution ran and worked.
pub const EXECUTION_SUCCEEDED: &str = "execution.succeeded";
/// An execution ran and did not work.
pub const EXECUTION_FAILED: &str = "execution.failed";
/// An execution may have run; nobody knows.
pub const EXECUTION_OUTCOME_UNKNOWN: &str = "execution.outcome_unknown";
/// An execution never ran.
pub const EXECUTION_NOT_RUN: &str = "execution.not_run";
/// An execution could not run because its host or capability was not available.
pub const EXECUTION_UNAVAILABLE: &str = "execution.unavailable";
/// An execution was refused before it was approved or dispatched.
pub const EXECUTION_REFUSED: &str = "execution.refused";

/// What a settlement's reason starts with when the loop guard stopped the dispatch. The
/// decision function files such a refusal under [`EXECUTION_BLOCKED`] instead of
/// [`EXECUTION_REFUSED`].
pub const LOOP_GUARD_REASON: &str = "loop guard:";
