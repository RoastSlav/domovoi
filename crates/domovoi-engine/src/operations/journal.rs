//! Journal payload builders for the operation kinds (payload version 1).
//!
//! No payload ever carries a step's output, a secret or a full plan hash: a plan hash is
//! shown as its first 8 hex digits.

use domovoi_core::{
    ExecutionState, Operation, OperationStatus, Step, StepId, StepIntent, StepProgress,
};
use serde_json::{Value, json};

/// The payload version every operation journal entry carries.
pub const PAYLOAD_V: u32 = 1;

/// Payload of `operation.created`.
#[must_use]
pub fn operation_created(operation: &Operation, incidents: &[String]) -> Value {
    json!({
        "operation": operation.id.to_string(),
        "kind": operation.kind.as_str(),
        "goal": operation.goal,
        "incidents": incidents,
        "parent": operation.parent.map(|p| p.to_string()),
    })
}

/// Payload of `operation.status`. `summary` is present only when a person or the model
/// finished the operation with one.
#[must_use]
pub fn operation_status(
    from: &OperationStatus,
    to: &OperationStatus,
    cycle: u32,
    summary: Option<&str>,
) -> Value {
    let mut payload = json!({ "from": from.name(), "to": to.name(), "cycle": cycle });
    if let Some(reason) = to.pause_reason() {
        payload["pause_reason"] = serde_json::to_value(reason).unwrap_or(Value::Null);
    }
    if let Some(summary) = summary {
        payload["summary"] = json!(summary);
    }
    payload
}

/// Payload of `operation.step_created`.
#[must_use]
pub fn step_created(step: &Step) -> Value {
    json!({
        "step": step.id.to_string(),
        "ordinal": step.ordinal,
        "kind": step.kind.as_str(),
        "phase": step.phase,
        "parent": step.parent.map(|p| p.to_string()),
        "cycle": step.cycle,
        "intent": intent(&step.intent),
    })
}

/// Payload of `operation.step_started`.
#[must_use]
pub fn step_started(step: StepId) -> Value {
    json!({ "step": step.to_string() })
}

/// Payload of `operation.step_finished`. Never carries the step's output.
#[must_use]
pub fn step_finished(step: StepId, progress: &StepProgress) -> Value {
    let mut payload = json!({
        "step": step.to_string(),
        "outcome": progress.outcome.as_str(),
    });
    if let Some(summary) = &progress.summary {
        payload["summary"] = json!(summary);
    }
    if let Some(reason) = &progress.reason {
        payload["reason"] = json!(reason);
    }
    payload
}

/// Payload of `operation.execution`.
#[must_use]
pub fn execution_transition(
    step: StepId,
    from: ExecutionState,
    to: ExecutionState,
    reason: Option<&str>,
) -> Value {
    let mut payload = json!({
        "step": step.to_string(),
        "execution": step.to_string(),
        "from": from.name(),
        "to": to.name(),
    });
    if let Some(reason) = reason {
        payload["reason"] = json!(reason);
    }
    payload
}

/// A step's intent as journal JSON: the serialized intent with an approval's plan hash cut
/// to its first 8 hex digits.
fn intent(intent: &StepIntent) -> Value {
    let mut value = serde_json::to_value(intent).unwrap_or(Value::Null);
    if let StepIntent::Approval { plan_hash, .. } = intent {
        value["plan_hash"] = json!(plan_hash.short());
    }
    value
}
