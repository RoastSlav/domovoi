//! The effects a decision creates: payloads, dedup keys and attempt limits in one place.
//!
//! Every payload is JSON with `"v": 1`. Handlers read steps and executions from the store,
//! never from the payload; the ids are only the address.

use domovoi_core::{OperationId, StepId};
use serde_json::{Value, json};

/// Attempts allowed to collect evidence.
pub(super) const COLLECT_ATTEMPTS: u32 = 3;
/// Attempts allowed to dispatch an approved execution.
pub(super) const DISPATCH_ATTEMPTS: u32 = 5;
/// Attempts allowed to reconcile a dispatched execution.
pub(super) const RECONCILE_ATTEMPTS: u32 = 5;
/// Attempts allowed to re-run a check for a verification.
pub(super) const VERIFY_ATTEMPTS: u32 = 5;
/// Attempts allowed to a timed wake.
pub(super) const WAKE_ATTEMPTS: u32 = 3;
/// Attempts allowed to build the context of a new cycle.
pub(super) const BUILD_CONTEXT_ATTEMPTS: u32 = 3;

/// Payload of `collect_evidence`, `dispatch` and `reconcile`.
pub(super) fn step_payload(operation: OperationId, step: StepId) -> Value {
    json!({ "v": 1, "operation": operation.to_string(), "step": step.to_string() })
}

/// Payload of `verify`: the Verification step to settle and the Execution step it checks.
pub(super) fn verify_payload(
    operation: OperationId,
    verification: StepId,
    execution: StepId,
) -> Value {
    json!({
        "v": 1,
        "operation": operation.to_string(),
        "step": verification.to_string(),
        "execution": execution.to_string(),
    })
}

/// Payload of `wake`; `step` is absent for the wall-time wake.
pub(super) fn wake_payload(operation: OperationId, step: Option<StepId>, purpose: &str) -> Value {
    let mut payload = json!({ "v": 1, "operation": operation.to_string(), "purpose": purpose });
    if let Some(step) = step {
        payload["step"] = json!(step.to_string());
    }
    payload
}

/// Payload of `build_context` for the start of `cycle`.
pub(super) fn build_payload(operation: OperationId, cycle: u32) -> Value {
    json!({ "v": 1, "operation": operation.to_string(), "cycle": cycle })
}

/// Dedup key of the evidence collection for `step`.
pub(super) fn collect_key(step: StepId) -> String {
    format!("collect:{step}")
}

/// Dedup key of the dispatch of Execution step `step`.
pub(super) fn dispatch_key(step: StepId) -> String {
    format!("dispatch:{step}")
}

/// Dedup key of the reconcile of Execution step `step`.
pub(super) fn reconcile_key(step: StepId) -> String {
    format!("reconcile:{step}")
}

/// Dedup key of the verification that follows Execution step `step`.
pub(super) fn verify_key(step: StepId) -> String {
    format!("verify:{step}")
}

/// Dedup key of the approval-expiry wake of Execution step `step`.
pub(super) fn expiry_key(step: StepId) -> String {
    format!("wake:expiry:{step}")
}

/// Dedup key of the timeout wake of Question step `step`.
pub(super) fn question_key(step: StepId) -> String {
    format!("wake:question:{step}")
}

/// Dedup key of the wall-time wake for `operation` at a limit of `seconds`.
pub(super) fn wall_key(operation: OperationId, seconds: u64) -> String {
    format!("wake:wall:{operation}:{seconds}")
}

/// Dedup key of the context build that starts `cycle`.
pub(super) fn build_key(operation: OperationId, cycle: u32) -> String {
    format!("build:{operation}:{cycle}")
}
