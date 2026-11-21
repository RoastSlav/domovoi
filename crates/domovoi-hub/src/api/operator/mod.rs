//! The Operator: durable, read-only conversations about the hosts.
//!
//! A question is committed together with a pending answer and answers `202` at once; the
//! model runs in a background task of the conversation service, and the finished answer is a
//! journal event. While it is written, the answer streams as ephemeral `live.*` events.

mod context;
mod create;
mod dto;
pub mod live;
mod messages;

use std::collections::HashMap;

use domovoi_core::conversation::Conversation;
use domovoi_core::ports::Versioned;
use domovoi_core::{ConversationId, HostId, OperationId, TurnId};
use domovoi_engine::conversations::ConversationError;

pub(super) use self::create::*;
pub(super) use self::messages::*;
use super::ApiState;
use super::error::ApiError;

/// Maps a conversation use-case failure to the API's error.
pub(super) fn conversation_error(err: ConversationError) -> ApiError {
    match err {
        ConversationError::NotFound => ApiError::NotFound,
        ConversationError::TurnInProgress => {
            ApiError::Conflict("an answer is already being written; stop it or wait".to_owned())
        }
        ConversationError::Full => ApiError::Conflict(err.to_string()),
        ConversationError::NotRetryable => ApiError::Conflict(err.to_string()),
        ConversationError::BadQuestion => ApiError::BadRequest(err.to_string()),
        ConversationError::Store(detail) => ApiError::internal("conversation failed", detail),
        ConversationError::Unavailable => ApiError::Unavailable("the hub is stopping".to_owned()),
    }
}

/// A conversation id from a path, or 404.
pub(super) fn conversation_id(text: &str) -> Result<ConversationId, ApiError> {
    text.parse().map_err(|_| ApiError::NotFound)
}

/// A turn id from a body or path, or 400.
pub(super) fn turn_id(text: &str) -> Result<TurnId, ApiError> {
    text.parse()
        .map_err(|_| ApiError::BadRequest("the turn must be a uuid".to_owned()))
}

/// An operation id, or 400.
pub(super) fn operation_id(text: &str) -> Result<OperationId, ApiError> {
    text.parse()
        .map_err(|_| ApiError::BadRequest("the operation id must be a uuid".to_owned()))
}

/// The stored conversation, or 404.
pub(super) async fn stored(
    state: &ApiState,
    id: ConversationId,
) -> Result<Versioned<Conversation>, ApiError> {
    state
        .deps
        .conversations
        .get(id)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Fails with 400/404 unless an operation with this id exists.
pub(super) async fn require_operation(state: &ApiState, id: OperationId) -> Result<(), ApiError> {
    state
        .deps
        .operation_repo
        .get(id)
        .await?
        .map(|_| ())
        .ok_or(ApiError::NotFound)
}

/// Host ids to their current names, read once per response.
pub(super) async fn host_names(state: &ApiState) -> Result<HashMap<HostId, String>, ApiError> {
    Ok(state
        .deps
        .hosts
        .list_hosts()
        .await?
        .into_iter()
        .map(|host| (host.id, host.name.to_string()))
        .collect())
}
