//! Lookups and checks shared by the approval decisions. Only the writer task
//! applies them, and approval is still the only way into `Approved`.

use domovoi_core::{ActionId, ActionRequest};

use super::{ApprovalError, Shared};
use crate::triage::token_hash;

/// Logs one refusal at the level it deserves. The caller gets every one of these back, so a
/// missing action, a used chat token, an action that already has an outcome and a shutdown
/// are debug: the person sees why nothing happened. A store failure warns because the answer
/// the caller gets says only that the store failed, and the reason is worth a line in the log.
pub(crate) fn refused(err: &ApprovalError, what: &str) {
    match err {
        ApprovalError::Store(why) => tracing::warn!(%why, "the store refused {what}"),
        ApprovalError::NotFound(id) => {
            tracing::debug!(%id, "{what} asked for an action that is not there")
        }
        ApprovalError::BadToken => tracing::debug!("{what} had no working chat token"),
        ApprovalError::PlanChanged => tracing::debug!("{what} named a different plan"),
        ApprovalError::NotYetSupported => {
            tracing::debug!("{what} carried a policy authorization, which is not supported yet")
        }
        ApprovalError::MissingAuthorization => {
            tracing::debug!("{what} had no valid human authorization")
        }
        ApprovalError::InconsistentAuthorization => {
            tracing::debug!("{what} had approval evidence that contradicts its plan")
        }
        ApprovalError::Refused(reason) => {
            tracing::debug!(%reason, "the action could not take {what}")
        }
        ApprovalError::StaleEffect => {
            tracing::warn!("{what} belonged to a stale lease holder and was discarded")
        }
        ApprovalError::Unavailable => tracing::debug!("{what} did not reach the writer"),
    }
}

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
