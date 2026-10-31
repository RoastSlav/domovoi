//! The incident state machine as a pure function.

use jiff::Timestamp;

use super::state::{Incident, IncidentPolicy, IncidentStatus};
use crate::error::TransitionError;
use crate::id::UserId;
use crate::severity::Severity;
use crate::signal::SignalClass;

/// Something that happened to an incident.
#[derive(Debug, Clone, PartialEq)]
pub enum IncidentEvent {
    /// A firing signal arrived for this fingerprint.
    Firing {
        /// Severity of the signal.
        severity: Severity,
        /// Whether the signal skips confirmation.
        class: SignalClass,
        /// When the check observed the condition.
        observed_at: Timestamp,
        /// The tracked numeric value, if the signal carried one.
        value: Option<f64>,
    },
    /// A resolved signal arrived for this fingerprint.
    Resolved {
        /// When the check observed the condition clear.
        observed_at: Timestamp,
    },
    /// A human acknowledged the incident.
    Ack {
        /// Who acknowledged it.
        by: UserId,
    },
    /// A human muted the incident.
    Mute {
        /// When the mute ends.
        until: Timestamp,
    },
    /// A human lifted a mute early.
    Unmute,
    /// Time passed; expires mutes and applies the TTL.
    Tick,
}

/// What an applied event did, for the caller to persist and publish.
#[derive(Debug, Clone, PartialEq)]
pub enum IncidentChange {
    /// A pending incident was confirmed.
    Confirmed,
    /// The status changed.
    StatusChanged {
        /// Status before.
        from: IncidentStatus,
        /// Status after.
        to: IncidentStatus,
    },
    /// The severity rose.
    SeverityChanged {
        /// Severity before.
        from: Severity,
        /// Severity after.
        to: Severity,
    },
    /// Another firing was folded into an open incident.
    Folded {
        /// Total occurrences after folding.
        occurrences: u32,
    },
    /// A resolved incident fired again inside the reopen window.
    Reopened,
    /// A pending incident resolved before confirmation; the caller deletes it.
    Dropped,
    /// A tracked value was added to the history.
    ValueRecorded {
        /// The recorded value.
        value: f64,
    },
}

fn set_status(inc: &mut Incident, to: IncidentStatus, changes: &mut Vec<IncidentChange>) {
    let from = inc.status;
    inc.status = to;
    changes.push(IncidentChange::StatusChanged { from, to });
}

/// Records what a firing signal contributes: time, value and a possibly higher severity.
fn absorb_firing(
    inc: &mut Incident,
    severity: Severity,
    observed_at: Timestamp,
    value: Option<f64>,
    changes: &mut Vec<IncidentChange>,
) {
    inc.occurrences = inc.occurrences.saturating_add(1);
    inc.last_firing_at = inc.last_firing_at.max(observed_at);
    if let Some(value) = value.filter(|v| v.is_finite()) {
        inc.history.push(observed_at, value);
        changes.push(IncidentChange::ValueRecorded { value });
    }
    if severity > inc.severity {
        changes.push(IncidentChange::SeverityChanged {
            from: inc.severity,
            to: severity,
        });
        inc.severity = severity;
    }
}

/// Applies `event` to `incident`, returning what changed.
///
/// This is the only place an incident's status changes. On error the incident is left
/// unchanged. See `ARCHITECTURE.md` section 5.3 for the state diagram.
pub fn apply(
    incident: &mut Incident,
    event: IncidentEvent,
    policy: &IncidentPolicy,
    now: Timestamp,
) -> Result<Vec<IncidentChange>, TransitionError> {
    let mut changes = Vec::new();
    match (incident.status, &event) {
        (
            IncidentStatus::Pending,
            &IncidentEvent::Firing {
                severity,
                class,
                observed_at,
                value,
            },
        ) => {
            incident.firing_streak = incident.firing_streak.saturating_add(1);
            absorb_firing(incident, severity, observed_at, value, &mut changes);
            if class == SignalClass::Immediate || incident.firing_streak >= policy.confirm_runs {
                incident.confirmed_at = Some(now);
                changes.push(IncidentChange::Confirmed);
                set_status(incident, IncidentStatus::Open, &mut changes);
            }
        }
        (IncidentStatus::Pending, IncidentEvent::Resolved { .. }) => {
            changes.push(IncidentChange::Dropped);
        }
        (
            IncidentStatus::Open | IncidentStatus::Acked | IncidentStatus::Muted { .. },
            &IncidentEvent::Firing {
                severity,
                observed_at,
                value,
                ..
            },
        ) => {
            absorb_firing(incident, severity, observed_at, value, &mut changes);
            changes.push(IncidentChange::Folded {
                occurrences: incident.occurrences,
            });
        }
        (
            IncidentStatus::Open | IncidentStatus::Acked | IncidentStatus::Muted { .. },
            IncidentEvent::Resolved { .. },
        ) => resolve(incident, now, &mut changes),
        (
            IncidentStatus::Resolved,
            &IncidentEvent::Firing {
                severity,
                observed_at,
                value,
                ..
            },
        ) => {
            let resolved_at = incident.resolved_at.unwrap_or(now);
            if now.duration_since(resolved_at) > policy.reopen_window {
                return Err(TransitionError::ReopenWindowElapsed);
            }
            incident.severity = severity;
            incident.resolved_at = None;
            absorb_firing(incident, severity, observed_at, value, &mut changes);
            changes.push(IncidentChange::Reopened);
            set_status(incident, IncidentStatus::Open, &mut changes);
        }
        (IncidentStatus::Open | IncidentStatus::Muted { .. }, IncidentEvent::Ack { .. }) => {
            set_status(incident, IncidentStatus::Acked, &mut changes);
        }
        (IncidentStatus::Open | IncidentStatus::Acked, &IncidentEvent::Mute { until }) => {
            set_status(incident, IncidentStatus::Muted { until }, &mut changes);
        }
        (IncidentStatus::Muted { .. }, IncidentEvent::Unmute) => {
            set_status(incident, IncidentStatus::Open, &mut changes);
        }
        (
            IncidentStatus::Open | IncidentStatus::Acked | IncidentStatus::Muted { .. },
            IncidentEvent::Tick,
        ) => tick(incident, policy, now, &mut changes),
        (IncidentStatus::Pending | IncidentStatus::Resolved, IncidentEvent::Tick) => {}
        (status, _) => return Err(TransitionError::Illegal { status, event }),
    }
    Ok(changes)
}

fn resolve(incident: &mut Incident, now: Timestamp, changes: &mut Vec<IncidentChange>) {
    incident.resolved_at = Some(now);
    set_status(incident, IncidentStatus::Resolved, changes);
}

fn tick(
    incident: &mut Incident,
    policy: &IncidentPolicy,
    now: Timestamp,
    changes: &mut Vec<IncidentChange>,
) {
    let quiet_too_long = policy
        .ttl
        .and_then(|ttl| incident.last_firing_at.checked_add(ttl).ok())
        .is_some_and(|deadline| deadline <= now);
    if quiet_too_long {
        resolve(incident, now, changes);
    } else if let IncidentStatus::Muted { until } = incident.status
        && until <= now
    {
        set_status(incident, IncidentStatus::Open, changes);
    }
}
