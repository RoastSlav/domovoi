//! Whether an approved action is still worth dispatching.
//!
//! An approval can wait: a hub may restart while the host's agent is gone, and the agent
//! may take a while to come back. By the time the action would reach the agent, the
//! approval's window may have closed or the incident it was approved for may be gone or
//! already resolved. Dispatching then would run a skill nobody can still want, so the
//! executor and the writer both ask this module first. A store that cannot answer is an
//! error, not a pass: the action stays approved and the next attempt asks again.

use domovoi_core::ports::StoreError;
use domovoi_core::{ActionRequest, Incident, IncidentStatus};
use jiff::Timestamp;

use super::Shared;

/// Why an approved action must not be dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stale {
    /// The approval's window closed while the action waited.
    Expired,
    /// The incident the action belongs to is no longer in the store.
    IncidentGone,
    /// The incident was resolved while the action waited.
    IncidentResolved,
}

impl Stale {
    /// The sentence the person is shown when the action is rejected for this reason.
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Stale::Expired => "the approval expired before the action was dispatched",
            Stale::IncidentGone => "the incident no longer exists",
            Stale::IncidentResolved => "the incident was resolved before the action was dispatched",
        }
    }
}

/// The incident an approved action is still worth dispatching for, or why it is not.
pub(crate) enum Fresh {
    /// Still worth dispatching; the incident read is handed to the caller so a v2 target
    /// can be recomputed from the same read.
    Current(Box<Incident>),
    /// Not worth dispatching any more.
    Stale(Stale),
}

/// Whether `action` may still be dispatched at `now`, and the incident it was checked
/// against when it may.
///
/// The incident is read rather than remembered, so an incident resolved by a signal
/// while the hub was down is seen here too.
pub(crate) async fn check(
    shared: &Shared,
    action: &ActionRequest,
    now: Timestamp,
) -> Result<Fresh, StoreError> {
    if now >= action.expires_at() {
        return Ok(Fresh::Stale(Stale::Expired));
    }
    match shared.deps.incidents.get(action.incident()).await? {
        None => Ok(Fresh::Stale(Stale::IncidentGone)),
        Some(incident) if incident.status() == IncidentStatus::Resolved => {
            Ok(Fresh::Stale(Stale::IncidentResolved))
        }
        Some(incident) => Ok(Fresh::Current(Box::new(incident))),
    }
}

#[cfg(test)]
#[path = "freshness_tests.rs"]
mod tests;
