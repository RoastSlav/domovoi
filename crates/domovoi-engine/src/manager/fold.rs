//! Small helpers the decision function shares: names, and the journal row of a fold.

use domovoi_core::ports::{AggregateRef, JournalDraft, JournalKind};
use domovoi_core::{Actor, Incident, IncidentStatus, SYSTEM_INCIDENTS, Signal};
use jiff::Timestamp;
use serde_json::json;

/// The status as a short lowercase word, for timeline summaries.
pub(super) fn status_name(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Pending => "pending",
        IncidentStatus::Open => "open",
        IncidentStatus::Acked => "acked",
        IncidentStatus::Muted { .. } => "muted",
        IncidentStatus::Resolved => "resolved",
    }
}

/// The journal row written in the same commit as a fold: which signal joined and how many
/// occurrences the incident has now.
pub(super) fn folded_row(
    incident: &Incident,
    signal: &Signal,
    occurrences: u32,
    at: Timestamp,
) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::incident(incident.id()),
        kind: JournalKind::IncidentFolded,
        payload_v: 1,
        payload: json!({"signal": signal.id().to_string(), "occurrences": occurrences}),
        actor: Actor::System(SYSTEM_INCIDENTS.to_owned()),
        occurred_at: at,
    }
}
