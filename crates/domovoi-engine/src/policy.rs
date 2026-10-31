//! Tunables of the incident manager.

use domovoi_core::IncidentPolicy;
use jiff::SignedDuration;

/// How the incident manager confirms, groups and expires incidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnginePolicy {
    /// The state machine's confirmation runs, reopen window and TTL.
    pub incident: IncidentPolicy,
    /// How recently an incident from another source must have opened for a signal about
    /// the same target to join it.
    pub grouping_window: SignedDuration,
    /// How often the manager ticks, expiring mutes and resolving quiet incidents.
    pub tick_every: SignedDuration,
}

impl Default for EnginePolicy {
    fn default() -> Self {
        Self {
            incident: IncidentPolicy::default(),
            grouping_window: SignedDuration::from_mins(5),
            tick_every: SignedDuration::from_secs(30),
        }
    }
}
