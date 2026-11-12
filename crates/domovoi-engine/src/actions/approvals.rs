//! Lookups and checks shared by the approval decisions. Only the writer task
//! applies them, and approval is still the only way into `Approved`.

use domovoi_core::{ActionId, ActionRequest};

use super::{ApprovalError, Shared};
use crate::triage::token_hash;

pub(crate) fn store(err: impl std::fmt::Display) -> ApprovalError {
    ApprovalError::Store(err.to_string())
}

/// Whether two digests are equal, in time independent of where they differ.
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub(crate) async fn load(shared: &Shared, id: ActionId) -> Result<ActionRequest, ApprovalError> {
    shared
        .deps
        .actions
        .get(id)
        .await
        .map_err(store)?
        .ok_or(ApprovalError::NotFound(id))
}

/// Whether a chat approval carries the action's own single-use token.
pub(crate) fn chat_token_ok(chat_token: Option<&str>, action: &ActionRequest) -> bool {
    match (chat_token, action.chat_token_hash()) {
        (Some(token), Some(stored)) => same(&token_hash(token), stored),
        _ => false,
    }
}
