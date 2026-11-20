//! The one post-step of `decide` that queues a resolution summary.
//!
//! A summary is written by an effect after the operation ends, never inside `decide`: the
//! decision is pure and holds no redactor. The terminal commit creates the effect, so it can
//! not be forgotten.

use domovoi_core::ports::EffectKind;
use domovoi_core::{OperationId, StepId, StepOutcome};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::{Decision, EffectSpec, IdSource, OperationEvent};
use crate::operations::state::OperationState;

/// Attempts allowed to draft a summary.
pub(crate) const RESOLUTION_ATTEMPTS: u32 = 5;

/// Payload of `draft_resolution`.
pub(crate) fn resolution_payload(operation: OperationId) -> Value {
    json!({ "v": 1, "operation": operation.to_string() })
}

/// Dedup key of the summary of a finished operation.
pub(crate) fn resolution_key(operation: OperationId) -> String {
    format!("resolution:{operation}")
}

/// Dedup key of the summary update after a late result of `step`.
fn late_key(operation: OperationId, step: StepId) -> String {
    format!("resolution:{operation}:{step}")
}

/// The step of an event that records the end of a run on an operation that was already over
/// and whose step was still running, `None` for every other event.
pub(super) fn late_step(state: &OperationState, event: &OperationEvent) -> Option<StepId> {
    let step = match event {
        OperationEvent::ExecutionSettled { step, .. } | OperationEvent::Verified { step, .. } => {
            *step
        }
        _ => return None,
    };
    let running = state
        .step(step)
        .is_some_and(|record| record.progress.outcome == StepOutcome::Running);
    (state.operation().status.is_terminal() && running).then_some(step)
}

/// Adds the `draft_resolution` effect when the operation ended in this decision, or when a
/// late result was recorded on one that had already ended.
pub(super) fn add_resolution_effect(
    state: &OperationState,
    late: Option<StepId>,
    decision: &mut Decision,
    ids: &mut dyn IdSource,
    now: Timestamp,
) {
    let operation = decision.operation.id;
    let key = if !state.operation().status.is_terminal() && decision.operation.status.is_terminal()
    {
        resolution_key(operation)
    } else if let Some(step) = late {
        late_key(operation, step)
    } else {
        return;
    };
    decision.effects.push(EffectSpec {
        id: ids.effect(),
        kind: EffectKind::DraftResolution,
        payload: resolution_payload(operation),
        not_before: now,
        max_attempts: RESOLUTION_ATTEMPTS,
        dedup_key: Some(key),
    });
}
