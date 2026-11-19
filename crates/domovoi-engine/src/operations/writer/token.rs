//! Replacing the chat token of a step that still waits for approval.

use domovoi_core::ports::{AuditDraft, ExecutionGuard, OperationWrite, StateWrite, Transaction};
use domovoi_core::{ExecutionState, OperationId, StepId};

use super::OpShared;
use super::execution::system;
use crate::operations::audit;
use crate::operations::decide::DecideError;
use crate::operations::error::OperationError;
use crate::operations::state::OperationState;

/// Stores `hash` as the step's chat token. Refused when the step is not awaiting approval
/// (`StepNotOpen`), when its window closed (`Expired`) or when it is missing (`NotFound`).
pub(super) async fn rotate(
    shared: &OpShared,
    operation: OperationId,
    step: StepId,
    hash: [u8; 32],
) -> Result<(), OperationError> {
    let deps = &shared.deps;
    let now = deps.clock.now();
    let snapshot = deps
        .operations
        .load(operation)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?
        .ok_or(OperationError::NotFound(operation))?;
    let state = OperationState::new(snapshot);
    let Some(row) = state.snapshot().execution_of(step) else {
        return Err(DecideError::UnknownStep(step).into());
    };
    if state.execution_state(step) != Some(ExecutionState::AwaitingApproval) {
        return Err(DecideError::StepNotOpen(step).into());
    }
    if now >= row.value.expires_at {
        return Err(domovoi_core::ExecutionError::Expired.into());
    }
    let version = row.version;
    let mut record = row.value.clone().with_chat_token_hash(hash);
    record.updated_at = now;
    let audit_row = AuditDraft::new(
        system(),
        now,
        audit::EXECUTION_TOKEN_ROTATED.to_owned(),
        format!("execution:{}", record.execution),
        serde_json::json!({ "operation": operation.to_string(), "step": step.to_string() }),
    );
    let tx = Transaction::new()
        .write(StateWrite::Operation(OperationWrite::Execution {
            execution: record,
            expected: ExecutionGuard::Version(version),
        }))
        .audit(audit_row);
    super::commit::commit_plain(shared, tx).await
}
