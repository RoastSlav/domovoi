//! Whether a change is told to people. Pure. Suppressed incidents still change state and
//! are still stored; only their events carry `notify: false`.

use domovoi_core::{AckRule, AckRuleId, HostId, Incident, MaintenanceWindow, Signal};
use jiff::Timestamp;

/// Why a change is not notified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressReason {
    /// The host is in a maintenance window.
    Maintenance,
    /// An ack rule matches the signal.
    Ack(AckRuleId),
}

/// Whether a maintenance window covers `host` at `now`.
#[must_use]
pub fn in_maintenance(host: HostId, windows: &[MaintenanceWindow], now: Timestamp) -> bool {
    windows.iter().any(|w| w.host == host && w.covers(now))
}

/// Why the change `signal` makes to `incident` should not be notified, if it should not.
/// Maintenance is checked first.
#[must_use]
pub fn suppression(
    signal: &Signal,
    incident: &Incident,
    acks: &[AckRule],
    windows: &[MaintenanceWindow],
    now: Timestamp,
) -> Option<SuppressReason> {
    if in_maintenance(incident.host(), windows, now) {
        return Some(SuppressReason::Maintenance);
    }
    acks.iter()
        .find(|rule| rule.matches(signal, now))
        .map(|rule| SuppressReason::Ack(rule.id))
}
