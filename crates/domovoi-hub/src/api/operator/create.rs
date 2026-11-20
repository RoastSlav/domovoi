//! Creating, listing, reading, attaching and deleting conversations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use domovoi_core::conversation::ConversationScope;
use domovoi_core::ports::ConversationFilter;

use super::dto::{
    ConversationDetailDto, ConversationListQuery, ConversationPage, ConversationSummaryDto,
    CreateConversationRequest, ScopeRequest, SetOperationRequest,
};
use super::{
    conversation_error, conversation_id, host_names, operation_id, require_operation, stored,
};
use crate::api::error::{ApiError, ErrorBody};
use crate::api::snapshot::snapshot_seq;
use crate::api::{ApiState, Authed, host_named, page};

/// The scope a create request names, or 400 / 404.
async fn scope_of(
    state: &ApiState,
    scope: Option<ScopeRequest>,
) -> Result<ConversationScope, ApiError> {
    let scope = scope
        .ok_or_else(|| ApiError::BadRequest("choose a scope: one host or all hosts".to_owned()))?;
    match (scope.kind.as_str(), scope.host) {
        ("global", None) => Ok(ConversationScope::Global),
        ("global", Some(_)) => Err(ApiError::BadRequest(
            "a conversation about all hosts names no host".to_owned(),
        )),
        ("host", Some(name)) => Ok(ConversationScope::Host(host_named(state, &name).await?.id)),
        ("host", None) => Err(ApiError::BadRequest(
            "a conversation about one host needs its name".to_owned(),
        )),
        _ => Err(ApiError::BadRequest(
            "the scope must be `global` or `host`".to_owned(),
        )),
    }
}

/// Lists conversations, newest first.
#[utoipa::path(
    get, path = "/api/v1/conversations", operation_id = "list_conversations", tag = "operator",
    params(ConversationListQuery),
    responses(
        (status = 200, description = "A page of conversations", body = ConversationPage),
        (status = 400, description = "A filter is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(in crate::api) async fn list_conversations(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<ConversationListQuery>,
) -> Result<Json<ConversationPage>, ApiError> {
    let as_of_seq = snapshot_seq(&state).await?;
    let filter = ConversationFilter {
        host: match query.host.as_deref() {
            Some(name) => Some(host_named(&state, name).await?.id),
            None => None,
        },
        operation: query.operation.as_deref().map(operation_id).transpose()?,
    };
    let rows = state
        .deps
        .conversations
        .list(&filter, &page(query.cursor, query.limit))
        .await?;
    let names = host_names(&state).await?;
    Ok(Json(ConversationPage {
        items: rows
            .items
            .iter()
            .map(|row| ConversationSummaryDto::of_row(row, &names))
            .collect(),
        next_cursor: rows.next_cursor,
        as_of_seq,
    }))
}

/// Starts a conversation. The scope is required: one host or all hosts.
#[utoipa::path(
    post, path = "/api/v1/conversations", operation_id = "create_conversation", tag = "operator",
    request_body = CreateConversationRequest,
    responses(
        (status = 201, description = "The new conversation", body = ConversationDetailDto),
        (status = 400, description = "The scope is missing or malformed", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host or operation", body = ErrorBody),
    )
)]
pub(in crate::api) async fn create_conversation(
    State(state): State<ApiState>,
    auth: Authed,
    Json(req): Json<CreateConversationRequest>,
) -> Result<(StatusCode, Json<ConversationDetailDto>), ApiError> {
    let scope = scope_of(&state, req.scope).await?;
    let operation = req.operation_id.as_deref().map(operation_id).transpose()?;
    if let Some(id) = operation {
        require_operation(&state, id).await?;
    }
    let as_of_seq = snapshot_seq(&state).await?;
    let created = state
        .services()
        .conversations
        .create(&auth.caller(), scope, operation)
        .await
        .map_err(conversation_error)?;
    let stored = stored(&state, created.id).await?;
    let names = host_names(&state).await?;
    Ok((
        StatusCode::CREATED,
        Json(ConversationDetailDto::of(
            &stored.value,
            stored.version,
            &[],
            &names,
            as_of_seq,
        )),
    ))
}

/// A conversation with all its kept messages.
#[utoipa::path(
    get, path = "/api/v1/conversations/{id}", operation_id = "get_conversation", tag = "operator",
    params(("id" = String, Path, description = "Conversation id")),
    responses(
        (status = 200, description = "The conversation", body = ConversationDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such conversation", body = ErrorBody),
    )
)]
pub(in crate::api) async fn get_conversation(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<ConversationDetailDto>, ApiError> {
    let id = conversation_id(&id)?;
    let as_of_seq = snapshot_seq(&state).await?;
    let stored = stored(&state, id).await?;
    let messages = state.deps.conversations.messages(id).await?;
    let names = host_names(&state).await?;
    Ok(Json(ConversationDetailDto::of(
        &stored.value,
        stored.version,
        &messages,
        &names,
        as_of_seq,
    )))
}

/// Attaches an operation to a conversation, or detaches it with `null`. The Operator only
/// reads a short summary of it.
#[utoipa::path(
    put, path = "/api/v1/conversations/{id}/operation", operation_id = "set_conversation_operation",
    tag = "operator",
    params(("id" = String, Path, description = "Conversation id")),
    request_body = SetOperationRequest,
    responses(
        (status = 200, description = "The updated conversation", body = ConversationSummaryDto),
        (status = 400, description = "The operation id is not a uuid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such conversation or operation", body = ErrorBody),
    )
)]
pub(in crate::api) async fn set_conversation_operation(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(req): Json<SetOperationRequest>,
) -> Result<Json<ConversationSummaryDto>, ApiError> {
    let id = conversation_id(&id)?;
    let operation = req.operation_id.as_deref().map(operation_id).transpose()?;
    if let Some(operation) = operation {
        require_operation(&state, operation).await?;
    }
    state
        .services()
        .conversations
        .set_operation(&auth.caller(), id, operation)
        .await
        .map_err(conversation_error)?;
    let stored = stored(&state, id).await?;
    let count =
        u32::try_from(state.deps.conversations.messages(id).await?.len()).unwrap_or(u32::MAX);
    let names = host_names(&state).await?;
    Ok(Json(ConversationSummaryDto::of(
        &stored.value,
        stored.version,
        count,
        &names,
    )))
}

/// Deletes a conversation and its messages. Refused while an answer is being written.
#[utoipa::path(
    delete, path = "/api/v1/conversations/{id}", operation_id = "delete_conversation", tag = "operator",
    params(("id" = String, Path, description = "Conversation id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such conversation", body = ErrorBody),
        (status = 409, description = "An answer is being written; stop it first", body = ErrorBody),
    )
)]
pub(in crate::api) async fn delete_conversation(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = conversation_id(&id)?;
    state
        .services()
        .conversations
        .delete(&auth.caller(), id)
        .await
        .map_err(|err| match err {
            domovoi_engine::conversations::ConversationError::TurnInProgress => {
                ApiError::Conflict("an answer is being written; stop it first".to_owned())
            }
            other => conversation_error(other),
        })?;
    Ok(StatusCode::NO_CONTENT)
}
