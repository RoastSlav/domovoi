//! What the incident manager can be asked to do, and why it can refuse.

use std::collections::BTreeMap;

use domovoi_core::{ActionId, IncidentId, Signal, TriageOutcome, UserId};
use jiff::Timestamp;

/// One request to the incident manager. Everything that changes an incident is one of these
/// (ADR 0011).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Signals to correlate, in order.
    Ingest(Vec<Signal>),
    /// A human acknowledged the incident.
    Ack {
        /// Which incident.
        incident: IncidentId,
        /// Who.
        by: UserId,
    },
    /// A human muted the incident.
    Mute {
        /// Which incident.
        incident: IncidentId,
        /// Until when.
        until: Timestamp,
        /// Who.
        by: UserId,
    },
    /// A human lifted a mute.
    Unmute {
        /// Which incident.
        incident: IncidentId,
    },
    /// Time passed: expire mutes and resolve incidents quiet past their TTL.
    Tick,
    /// Triage finished for an incident, which may have resolved meanwhile.
    AttachTriage {
        /// Which incident.
        incident: IncidentId,
        /// What triage produced.
        outcome: TriageOutcome,
        /// Plain chat tokens of the proposed actions, passed on in the event.
        tokens: BTreeMap<ActionId, String>,
    },
    /// A follow-up question and its answer, for the timeline. Publishes nothing.
    RecordFollowUp {
        /// Which incident.
        incident: IncidentId,
        /// Who asked.
        asked_by: UserId,
        /// The question.
        question: String,
        /// The redacted answer.
        answer: String,
    },
}

/// Why a command was not carried out. Nothing was stored and nothing was published.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// No active incident (or, for triage, no incident at all) has that id.
    #[error("no such incident {0}")]
    UnknownIncident(IncidentId),
    /// The incident cannot make that change from its current status.
    #[error("not allowed now: {0}")]
    NotAllowed(String),
    /// The store failed; the command can be retried.
    #[error("store error: {0}")]
    Store(String),
    /// The incident manager has stopped.
    #[error("the incident manager is not running")]
    Unavailable,
}
