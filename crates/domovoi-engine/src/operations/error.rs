//! Why a command to the operation service was refused.

use domovoi_core::{ExecutionError, HostId, OperationId};

use super::DecideError;

/// Why a command was refused. Nothing changed, except where the variant says otherwise.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OperationError {
    /// No operation has that id.
    #[error("no such operation {0}")]
    NotFound(OperationId),
    /// The decision function refused the event.
    #[error("{0}")]
    Decide(#[from] DecideError),
    /// The execution refused the authorization.
    #[error("{0}")]
    Execution(#[from] ExecutionError),
    /// The host has no live agent session, so a plan cannot be made for it.
    #[error("the host {0} has no connected agent")]
    HostUnavailable(HostId),
    /// The operation changed under the writer twice in a row.
    #[error("the operation changed meanwhile; try again")]
    Conflict,
    /// Another runner holds the effect this write was fenced by.
    #[error("the effect was claimed by someone else")]
    StaleEffect,
    /// A chat approval without the step's token, with a wrong one, or with one already used.
    #[error("the approval link is not valid")]
    BadToken,
    /// The store failed.
    #[error("store error: {0}")]
    Store(String),
    /// The service is shutting down; trying again after a moment works.
    #[error("the operation service is shutting down; try again")]
    Unavailable,
}
