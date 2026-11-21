//! Asking, stopping and retrying: the three calls that touch a running answer.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use domovoi_core::MessageId;
use domovoi_engine::conversations::TurnStarted;

use super::context::chat_context;
use super::dto::{CancelResponse, MessageDto, SendMessageRequest, SendResponse};
use super::{conversation_error, conversation_id, stored, turn_id};
use crate::api::error::{ApiError, ErrorBody};
use crate::api::snapshot::snapshot_seq;
use crate::api::{ApiState, Authed};

/// The response for a committed pair.
async fn sent(state: &ApiState, started: &TurnStarted) -> Result<SendResponse, ApiError> {
    Ok(SendResponse {
        user: MessageDto::from(&started.user),
        assistant: MessageDto::from(&started.assistant),
        as_of_seq: snapshot_seq(state).await?,
    })
}

/// Asks a question. The question and a pending answer are committed together and the call
/// answers at once; the answer is written in the background and arrives as a journal event,
/// preceded by `live.*` events on the event stream. Sending the same `turn` again returns the
/// stored pair and asks the model nothing.
#[utoipa::path(
    post, path = "/api/v1/conversations/{id}/messages", operation_id = "send_message", tag = "operator",
    params(("id" = String, Path, description = "Conversation id")),
    request_body = SendMessageRequest,
    responses(
        (status = 202, description = "Committed; the answer is being written", body = SendResponse),
        (status = 200, description = "This turn was sent before; the stored pair", body = SendResponse),
        (status = 400, description = "The question is empty or too long, or the turn is not a uuid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such conversation", body = ErrorBody),
        (status = 409, description = "An answer is being written, or the conversation is full", body = ErrorBody),
        (status = 503, description = "Triage is off", body = ErrorBody),
    )
)]
pub(in crate::api) async fn send_message(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendResponse>), ApiError> {
    let id = conversation_id(&id)?;
    let turn = turn_id(&req.turn)?;
    let text = req.text.trim().to_owned();
    if text.is_empty() || text.chars().count() > domovoi_core::conversation::MAX_QUESTION_CHARS {
        return Err(ApiError::BadRequest(format!(
            "the question must have 1 to {} characters",
            domovoi_core::conversation::MAX_QUESTION_CHARS
        )));
    }
    if state.deps.triage.is_none() {
        return Err(ApiError::TriageOff);
    }
    let conversation = stored(&state, id).await?;
    let context = chat_context(&state, &conversation.value).await?;
    let started = state
        .services()
        .conversations
        .send(&auth.caller(), id, turn, text, context)
        .await
        .map_err(conversation_error)?;
    let status = if started.replayed {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    Ok((status, Json(sent(&state, &started).await?)))
}

/// Stops the answer of a turn. A turn that already settled is left as it is.
#[utoipa::path(
    post, path = "/api/v1/conversations/{id}/turns/{turn}/cancel", operation_id = "cancel_turn",
    tag = "operator",
    params(
        ("id" = String, Path, description = "Conversation id"),
        ("turn" = String, Path, description = "The turn to stop"),
    ),
    responses(
        (status = 200, description = "The answer's status", body = CancelResponse),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such conversation or turn", body = ErrorBody),
    )
)]
pub(in crate::api) async fn cancel_turn(
    State(state): State<ApiState>,
    auth: Authed,
    Path((id, turn)): Path<(String, String)>,
) -> Result<Json<CancelResponse>, ApiError> {
    let id = conversation_id(&id)?;
    let turn = turn_id(&turn)?;
    let status = state
        .services()
        .conversations
        .cancel(&auth.caller(), id, turn)
        .await
        .map_err(conversation_error)?;
    Ok(Json(CancelResponse {
        status: status.as_str().to_owned(),
    }))
}

/// Asks the last failed or stopped question again, under a new turn.
#[utoipa::path(
    post, path = "/api/v1/conversations/{id}/messages/{message}/retry", operation_id = "retry_message",
    tag = "operator",
    params(
        ("id" = String, Path, description = "Conversation id"),
        ("message" = String, Path, description = "The assistant message to retry"),
    ),
    responses(
        (status = 202, description = "Committed; the answer is being written", body = SendResponse),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such conversation or message", body = ErrorBody),
        (status = 409, description = "Only the last answer can be retried, after it failed or was stopped", body = ErrorBody),
        (status = 503, description = "Triage is off", body = ErrorBody),
    )
)]
pub(in crate::api) async fn retry_message(
    State(state): State<ApiState>,
    auth: Authed,
    Path((id, message)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SendResponse>), ApiError> {
    let id = conversation_id(&id)?;
    let message: MessageId = message
        .parse()
        .map_err(|_| ApiError::BadRequest("the message must be a uuid".to_owned()))?;
    if state.deps.triage.is_none() {
        return Err(ApiError::TriageOff);
    }
    let conversation = stored(&state, id).await?;
    let context = chat_context(&state, &conversation.value).await?;
    let started = state
        .services()
        .conversations
        .retry(&auth.caller(), id, message, context)
        .await
        .map_err(conversation_error)?;
    Ok((StatusCode::ACCEPTED, Json(sent(&state, &started).await?)))
}
