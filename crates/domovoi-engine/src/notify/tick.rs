//! Time-driven notifications: reminders and releasing the held queue. Pure.

use std::fmt::Write;

use domovoi_core::{Incident, IncidentId, IncidentStatus, NotifyState, Severity};
use jiff::Timestamp;

use super::plan::NotifyAction;
use super::quiet::in_quiet_hours;
use super::settings::NotifySettings;
use super::text::human;

/// What a tick delivers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickPlan {
    /// Reminders, in order.
    pub actions: Vec<NotifyAction>,
    /// Incidents whose notification state changed, with the new state.
    pub states: Vec<(IncidentId, NotifyState)>,
    /// The single "while you were away" message, when quiet hours just ended.
    pub release: Option<String>,
}

fn reminder_every(incident: &Incident, settings: &NotifySettings) -> Option<jiff::SignedDuration> {
    match incident.severity() {
        Severity::Critical => Some(settings.remind_critical),
        Severity::Warning => Some(settings.remind_warning),
        Severity::Info => None,
    }
}

/// Reminders for open incidents and, outside quiet hours, the release of everything held.
///
/// `items` are the active and held incidents with their notification state.
#[must_use]
pub fn tick(
    items: &[(Incident, NotifyState)],
    settings: &NotifySettings,
    now: Timestamp,
) -> TickPlan {
    let quiet = in_quiet_hours(now, settings);
    let mut out = TickPlan::default();
    let mut held = Vec::new();
    for (incident, state) in items {
        if let Some(item) = &state.held {
            if !quiet {
                held.push(item.clone());
                let mut released = state.clone();
                released.held = None;
                out.states.push((incident.id(), released));
            }
            continue;
        }
        if incident.status() != IncidentStatus::Open || state.messages.is_empty() {
            continue;
        }
        let Some(every) = reminder_every(incident, settings) else {
            continue;
        };
        if quiet && incident.severity() < Severity::Critical {
            continue;
        }
        let Some(since) = state.last_reminder_at.or(state.first_alert_at) else {
            continue;
        };
        if now.duration_since(since) < every {
            continue;
        }
        let age = human(now.duration_since(incident.opened_at()));
        out.actions.push(NotifyAction::ThreadReply {
            incident: incident.id(),
            text: format!("Still ongoing ({age}): {}", incident.title()),
        });
        let mut reminded = state.clone();
        reminded.last_reminder_at = Some(now);
        out.states.push((incident.id(), reminded));
    }
    if !held.is_empty() {
        held.sort_by_key(|item| item.held_at);
        let mut text = format!(
            "While you were away: {} alert{}\n",
            held.len(),
            if held.len() == 1 { "" } else { "s" }
        );
        for item in &held {
            let resolved = if item.resolved { " (resolved)" } else { "" };
            _ = writeln!(text, "- {}{resolved}", item.line);
        }
        out.release = Some(text);
    }
    out
}
