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
    #[error("{}", illegal_text(*status, event))]
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

/// What to tell a person whose action cannot apply now, without internal names or ids.
fn illegal_text(status: IncidentStatus, event: &IncidentEvent) -> String {
    let clock = |at: jiff::Timestamp| at.strftime("%H:%M UTC").to_string();
    match (status, event) {
        (IncidentStatus::Pending, _) => {
            "This incident is not open yet; it is still waiting to be confirmed.".to_owned()
        }
        (IncidentStatus::Resolved, _) => "This incident is already resolved.".to_owned(),
        (IncidentStatus::Muted { until }, IncidentEvent::Mute { .. }) => {
            format!("Already muted until {}.", clock(until))
        }
        (IncidentStatus::Muted { until }, IncidentEvent::Ack { .. }) => {
            format!(
                "This incident is muted until {}; unmute it first.",
                clock(until)
            )
        }
        (IncidentStatus::Acked, IncidentEvent::Ack { .. }) => {
            "This incident is already acknowledged.".to_owned()
        }
        (_, IncidentEvent::Unmute) => "This incident is not muted.".to_owned(),
        _ => "That is not possible for this incident in its current state.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UserId;

    #[test]
    fn a_refused_action_is_plain_text_without_debug_names_or_ids() {
        let until: jiff::Timestamp = "2026-10-31T23:35:00Z".parse().unwrap();
        let cases = [
            (
                IncidentStatus::Muted { until },
                IncidentEvent::Mute { until },
                "Already muted until 23:35 UTC.",
            ),
            (
                IncidentStatus::Pending,
                IncidentEvent::Ack { by: UserId::new() },
                "This incident is not open yet; it is still waiting to be confirmed.",
            ),
            (
                IncidentStatus::Open,
                IncidentEvent::Unmute,
                "This incident is not muted.",
            ),
        ];
        for (status, event, text) in cases {
            let said = TransitionError::Illegal { status, event }.to_string();
            assert_eq!(said, text);
            assert!(!said.contains("UserId") && !said.contains('{'));
        }
    }
}
