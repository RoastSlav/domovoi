//! What to say about one domain event. Pure.

use domovoi_core::{ActionStatus, HeldItem, Incident, IncidentId, NotifyState, Severity};
use jiff::Timestamp;

use super::quiet::in_quiet_hours;
use super::settings::NotifySettings;
use super::text::{human, ordinal};
use crate::events::{DomainEvent, EventKind};

/// One thing to deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyAction {
    /// Post the incident's card and open its thread.
    PostCard {
        /// Which incident.
        incident: IncidentId,
    },
    /// Rebuild and edit the posted card.
    UpdateCard {
        /// Which incident.
        incident: IncidentId,
    },
    /// Post one line in the incident's thread.
    ThreadReply {
        /// Which incident.
        incident: IncidentId,
        /// The line.
        text: String,
    },
    /// Keep the alert for the end of quiet hours.
    Hold {
        /// Which incident.
        incident: IncidentId,
        /// What will be said then.
        item: HeldItem,
    },
}

/// The deliveries for one event and the notification state after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Deliver these, in order. Empty means say nothing.
    pub actions: Vec<NotifyAction>,
    /// Store this as the incident's new notification state.
    pub state: NotifyState,
}

/// The line an alert is summarised as when held.
#[must_use]
pub fn held_line(incident: &Incident) -> String {
    let severity = incident.severity().to_string();
    format!("[{severity}] {}", incident.title())
}

/// Whether a card for the incident was posted on some platform.
fn posted(state: &NotifyState) -> bool {
    !state.messages.is_empty()
}

/// A card change for an incident, posting the card instead when none is out yet and
/// nothing is held. Triage results and action changes use this too.
#[must_use]
pub fn card_changed(incident: &Incident, state: &NotifyState) -> Vec<NotifyAction> {
    let id = incident.id();
    if posted(state) {
        vec![NotifyAction::UpdateCard { incident: id }]
    } else if state.is_held() {
        Vec::new()
    } else {
        vec![NotifyAction::PostCard { incident: id }]
    }
}

fn opened(
    incident: &Incident,
    state: &mut NotifyState,
    settings: &NotifySettings,
    now: Timestamp,
    reopened: bool,
) -> Vec<NotifyAction> {
    let id = incident.id();
    if in_quiet_hours(now, settings) && incident.severity() < Severity::Critical {
        let item = HeldItem {
            line: held_line(incident),
            severity: incident.severity(),
            held_at: now,
            resolved: false,
        };
        state.held = Some(item.clone());
        state.first_alert_at.get_or_insert(now);
        return vec![NotifyAction::Hold { incident: id, item }];
    }
    state.held = None;
    state.last_reminder_at = None;
    state.first_alert_at = Some(now);
    if reopened && posted(state) {
        let text = format!("Back again: {}", incident.title());
        return vec![
            NotifyAction::UpdateCard { incident: id },
            NotifyAction::ThreadReply { incident: id, text },
        ];
    }
    vec![NotifyAction::PostCard { incident: id }]
}

fn folded(
    incident: &Incident,
    occurrences: u32,
    state: &mut NotifyState,
    settings: &NotifySettings,
    now: Timestamp,
) -> Vec<NotifyAction> {
    if !posted(state) {
        return Vec::new();
    }
    let due = state
        .last_fold_reply_at
        .is_none_or(|last| now.duration_since(last) >= settings.fold_every);
    if !due {
        state.folds_since_reply = state.folds_since_reply.saturating_add(1);
        return Vec::new();
    }
    let since = state.first_alert_at.unwrap_or(incident.opened_at());
    let more = match state.folds_since_reply {
        0 => String::new(),
        n => format!(", +{n} more"),
    };
    let text = format!(
        "Again: {} ({} time in {}{more})",
        incident.title(),
        ordinal(occurrences),
        human(now.duration_since(since))
    );
    state.last_fold_reply_at = Some(now);
    state.folds_since_reply = 0;
    vec![NotifyAction::ThreadReply {
        incident: incident.id(),
        text,
    }]
}

/// The thread reply for an action status that needs a person's attention.
fn action_problem(status: &ActionStatus) -> Option<String> {
    match status {
        ActionStatus::Rejected { reason } => Some(format!("The action was not run: {reason}.")),
        ActionStatus::Failed { reason } => Some(format!("The action failed: {reason}.")),
        ActionStatus::VerifyFailed => Some(
            "The action ran, but the check still fails. Look at the host before trying again."
                .to_owned(),
        ),
        _ => None,
    }
}

/// What to deliver for `event`, given what was said before (`state`).
#[must_use]
pub fn plan(
    event: &DomainEvent,
    state: &NotifyState,
    settings: &NotifySettings,
    now: Timestamp,
) -> Plan {
    let mut state = state.clone();
    if !event.notify {
        return Plan {
            actions: Vec::new(),
            state,
        };
    }
    let incident = &event.incident;
    let id = incident.id();
    let actions = match &event.kind {
        EventKind::IncidentOpened => opened(incident, &mut state, settings, now, false),
        EventKind::IncidentReopened => opened(incident, &mut state, settings, now, true),
        EventKind::IncidentFolded { occurrences, .. } => {
            folded(incident, *occurrences, &mut state, settings, now)
        }
        EventKind::IncidentEscalated { to, .. } => {
            if let Some(held) = &mut state.held {
                held.severity = *to;
                held.line = held_line(incident);
                if *to >= Severity::Critical {
                    // Critical alerts do not wait for quiet hours to end.
                    state.held = None;
                    state.first_alert_at = Some(now);
                    vec![NotifyAction::PostCard { incident: id }]
                } else {
                    Vec::new()
                }
            } else if posted(&state) {
                let text = format!("Escalated to {to}: {}", incident.title());
                vec![
                    NotifyAction::ThreadReply { incident: id, text },
                    NotifyAction::UpdateCard { incident: id },
                ]
            } else {
                card_changed(incident, &state)
            }
        }
        EventKind::IncidentResolved => {
            if let Some(held) = &mut state.held {
                held.resolved = true;
                Vec::new()
            } else if posted(&state) {
                let after = human(now.duration_since(incident.opened_at()));
                vec![
                    NotifyAction::ThreadReply {
                        incident: id,
                        text: format!("Resolved after {after}"),
                    },
                    NotifyAction::UpdateCard { incident: id },
                ]
            } else {
                Vec::new()
            }
        }
        EventKind::IncidentAcked { .. }
        | EventKind::IncidentMuted { .. }
        | EventKind::IncidentUnmuted => {
            if posted(&state) {
                vec![NotifyAction::UpdateCard { incident: id }]
            } else {
                Vec::new()
            }
        }
        EventKind::TriageRequested => Vec::new(),
        EventKind::TriageCompleted { .. } => card_changed(incident, &state),
        // The status change that follows updates the card.
        EventKind::ActionApproved { .. } => Vec::new(),
        EventKind::ActionChanged { status, .. } => {
            let mut actions = Vec::new();
            if let Some(text) = action_problem(status)
                && posted(&state)
            {
                actions.push(NotifyAction::ThreadReply { incident: id, text });
            }
            actions.extend(card_changed(incident, &state));
            actions
        }
        EventKind::ActionBlocked { reason, .. } => {
            let mut actions = Vec::new();
            if posted(&state) {
                actions.push(NotifyAction::ThreadReply {
                    incident: id,
                    text: format!(
                        "The action was not run: {reason}. Approve it again once the cause is clear."
                    ),
                });
            }
            actions.extend(card_changed(incident, &state));
            actions
        }
        EventKind::TriageDegraded { reason } => {
            if posted(&state) {
                vec![NotifyAction::ThreadReply {
                    incident: id,
                    text: format!(
                        "Triage is degraded: {reason}. Alerts still go out without triage."
                    ),
                }]
            } else {
                Vec::new()
            }
        }
    };
    Plan { actions, state }
}
