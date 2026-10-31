//! What the incident manager can be asked to do, and why it can refuse.

use domovoi_core::{IncidentId, Signal, UserId};
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
}

/// Why a command was not carried out. Nothing was stored and nothing was published.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// No active incident has that id.
    #[error("no active incident {0}")]
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
