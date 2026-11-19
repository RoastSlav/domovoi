//! What a person can do to an operation: cancel it, pause it, resume it and answer its
//! question. Each is human-only (a session and the CSRF header), goes through the operation
//! service, and answers with the refreshed operation and the journal position it was read at.

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::{OperationId, StepId};

use super::dto_operations::{
    AnswerStepRequest, BudgetExtensionDto, CancelOperationRequest, OperationControlDto,
    OperationSummaryDto, ResumeOperationRequest,
};
use super::error::{ApiError, ErrorBody};
use super::operation_errors::command_error;
use super::snapshot::snapshot_seq;
use super::{ApiState, Authed};
use crate::services::BudgetExtension;

/// The 503 every control answers while orchestration is off.
fn off() -> ApiError {
    ApiError::Unavailable("orchestration is not enabled".to_owned())
}

fn parse<T: std::str::FromStr>(text: &str) -> Result<T, ApiError> {
    text.parse().map_err(|_| ApiError::NotFound)
}

/// The refreshed operation. The journal head is read after the command and before the
/// operation, so `as_of_seq` is never ahead of what the summary shows.
async fn refreshed(
    state: &ApiState,
    id: OperationId,
) -> Result<Json<OperationControlDto>, ApiError> {
    let as_of_seq = snapshot_seq(state).await?;
    let snapshot = state
        .deps
        .operation_repo
        .load(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(OperationControlDto {
        as_of_seq,
        operation: OperationSummaryDto::of(&snapshot.operation, Some(&snapshot)),
    }))
}

/// Cancels an operation. Pending steps and effects are cancelled; a run already dispatched
/// is not recalled and its result is still recorded.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/cancel", operation_id = "cancel_operation", tag = "operations",
    params(("id" = String, Path, description = "Operation id")),
    request_body = CancelOperationRequest,
    responses(
        (status = 200, description = "Cancelled, or already cancelled", body = OperationControlDto),
        (status = 400, description = "The reason is too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 409, description = "The operation has already finished", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn cancel(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(req): Json<CancelOperationRequest>,
) -> Result<Json<OperationControlDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let id: OperationId = parse(&id)?;
    state
        .services()
        .operations
        .cancel(&auth.caller(), id, req.reason.as_deref())
        .await
        .map_err(command_error)?;
    refreshed(&state, id).await
}

/// Pauses an operation until a person resumes it.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/pause", operation_id = "pause_operation", tag = "operations",
    params(("id" = String, Path, description = "Operation id")),
    responses(
        (status = 200, description = "Paused, or already paused", body = OperationControlDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 409, description = "Not started yet, or already finished", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn pause(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<OperationControlDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let id: OperationId = parse(&id)?;
    state
        .services()
        .operations
        .pause(&auth.caller(), id)
        .await
        .map_err(command_error)?;
    refreshed(&state, id).await
}

/// Resumes a paused operation, raising the named limits first.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/resume", operation_id = "resume_operation", tag = "operations",
    params(("id" = String, Path, description = "Operation id")),
    request_body = ResumeOperationRequest,
    responses(
        (status = 200, description = "Resumed", body = OperationControlDto),
        (status = 400, description = "An unknown, repeated or out-of-range limit", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 409, description = "Not paused, or a spent limit is still spent", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn resume(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(req): Json<ResumeOperationRequest>,
) -> Result<Json<OperationControlDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let id: OperationId = parse(&id)?;
    let extend: Vec<BudgetExtension> = req
        .extend
        .into_iter()
        .map(|BudgetExtensionDto { kind, limit }| BudgetExtension { kind, limit })
        .collect();
    state
        .services()
        .operations
        .resume(&auth.caller(), id, &extend)
        .await
        .map_err(command_error)?;
    refreshed(&state, id).await
}

/// Answers the question an operation is waiting on.
#[utoipa::path(
    post, path = "/api/v1/operations/{id}/steps/{step}/answer", operation_id = "answer_operation_question", tag = "operations",
    params(
        ("id" = String, Path, description = "Operation id"),
        ("step" = String, Path, description = "Question step id"),
    ),
    request_body = AnswerStepRequest,
    responses(
        (status = 200, description = "Answered; the operation goes on", body = OperationControlDto),
        (status = 400, description = "The answer is empty or too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation or step", body = ErrorBody),
        (status = 409, description = "Not a question, not waiting, or the operation is paused or finished", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled, or the service is restarting", body = ErrorBody),
    )
)]
pub(super) async fn answer(
    State(state): State<ApiState>,
    auth: Authed,
    Path((id, step)): Path<(String, String)>,
    Json(req): Json<AnswerStepRequest>,
) -> Result<Json<OperationControlDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let (id, step): (OperationId, StepId) = (parse(&id)?, parse(&step)?);
    state
        .services()
        .operations
        .answer(&auth.caller(), id, step, &req.text)
        .await
        .map_err(command_error)?;
    refreshed(&state, id).await
}
