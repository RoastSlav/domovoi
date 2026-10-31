//! Error types for validation and incident transitions.

use crate::incident::{IncidentEvent, IncidentStatus};

/// A value failed validation when parsed into a domain type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {field}: {reason}")]
pub struct ValidationError {
    /// The name of the field or type that was rejected.
    pub field: &'static str,
    /// Why the value was rejected.
    pub reason: String,
}

impl ValidationError {
    /// Builds an error for `field` with a human-readable `reason`.
    pub fn new(field: &'static str, reason: impl Into<String>) -> Self {
        Self {
            field,
            reason: reason.into(),
        }
    }
}

/// An incident event could not be applied.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TransitionError {
    /// The event is not allowed in the incident's current status.
    #[error("event {event:?} is not allowed while incident is {status:?}")]
    Illegal {
        /// Status the incident was in.
        status: IncidentStatus,
        /// The rejected event.
        event: IncidentEvent,
    },
    /// A resolved incident fired again after its reopen window; the caller must create a new incident.
    #[error("reopen window elapsed; create a new incident")]
    ReopenWindowElapsed,
}
