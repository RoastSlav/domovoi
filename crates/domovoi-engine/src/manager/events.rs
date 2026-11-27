//! Security events on the timeline, and what time a report saw the incident at. Pure.

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::triage::{SECURITY_EVENT_KIND, SEEN_UNTIL_KEY, is_security_event};
use domovoi_core::{Incident, Signal, SignalStatus};
use jiff::Timestamp;
use serde_json::json;

use super::decide::{Builder, Decision, Write};
use crate::cards::{clip, evidence_text};
use crate::events::EventKind;

/// Longest timeline line of one event, in characters.
const MAX_SUMMARY_CHARS: usize = 500;

impl Builder<'_> {
    /// Asks for an investigation, unless the incident is a blind check: it already says why
    /// and how to fix it, and a model can read nothing the check could not.
    pub(super) fn request_triage(&mut self, incident: &Incident, notify: bool) {
        if !incident.is_monitoring_blind() {
            self.event(EventKind::TriageRequested, incident, notify);
        }
    }

    /// Puts a security event that joined `incident` on its timeline with its evidence.
    pub(super) fn security_event(&mut self, incident: &Incident, signal: &Signal) {
        if let Some(entry) = security_event(incident, signal, self.now) {
            self.out.writes.push(Write::Timeline(entry));
        }
    }
}

/// The timeline entry for `signal` joining `incident`, when it reports a security event:
/// what happened, its evidence and when the check saw it.
fn security_event(
    incident: &Incident,
    signal: &Signal,
    now: Timestamp,
) -> Option<IncidentTimelineEntry> {
    if signal.status() != SignalStatus::Firing || !is_security_event(signal) {
        return None;
    }
    let evidence: Vec<String> = signal
        .evidence()
        .iter()
        .map(|(key, value)| format!("{key}: {}", evidence_text(value)))
        .collect();
    let line = format!(
        "{} (seen {}; {})",
        signal.summary(),
        signal.observed_at().strftime("%Y-%m-%d %H:%M:%S UTC"),
        evidence.join(", ")
    );
    Some(IncidentTimelineEntry {
        incident: incident.id(),
        at: now,
        kind: SECURITY_EVENT_KIND.to_owned(),
        summary: clip(&line, MAX_SUMMARY_CHARS),
        detail: json!({
            "signal": signal.id(),
            "observed_at": signal.observed_at(),
            "evidence": signal.evidence(),
        }),
    })
}

/// Records on the report `decision` stores that its run read the incident at `seen`.
pub(super) fn mark_seen(decision: &mut Decision, seen: Timestamp) {
    for write in &mut decision.writes {
        if let Write::Timeline(entry) = write
            && matches!(entry.kind.as_str(), "triage.report" | "triage.cached")
            && let Some(detail) = entry.detail.as_object_mut()
        {
            detail.insert(SEEN_UNTIL_KEY.to_owned(), json!(seen));
        }
    }
}
