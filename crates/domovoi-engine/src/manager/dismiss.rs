//! Dismissing an incident: a human says the condition is not a real problem, so the
//! incident closes now and its verdict is `Expected`.
//!
//! The hub creates the ack rule for the check after the dismiss, so this decision writes
//! only the incident and its timeline.

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::{IncidentChange, IncidentEvent, IncidentId, UserId, Verdict, apply};
use jiff::Timestamp;
use serde_json::json;

use super::command::CommandError;
use super::decide::{Decision, State, Write, status_name};
use crate::events::{DomainEvent, EventKind};
use crate::policy::EnginePolicy;

/// Closes `incident` on behalf of `by`, recording `reason` in its timeline and setting the
/// verdict to `Expected`, so grouping files the incident under "being handled". The status
/// change publishes `IncidentResolved` like any other resolution, so cards, digests and
/// chat see it close.
pub(crate) fn dismiss(
    state: &State,
    incident: IncidentId,
    by: UserId,
    reason: &str,
    policy: &EnginePolicy,
    now: Timestamp,
) -> Result<Decision, CommandError> {
    let mut found = state
        .active
        .iter()
        .find(|i| i.id() == incident)
        .cloned()
        .ok_or(CommandError::UnknownIncident(incident))?;
    let changes = apply(
        &mut found,
        IncidentEvent::Dismiss { by },
        &policy.incident,
        now,
    )
    .map_err(|err| CommandError::NotAllowed(err.to_string()))?;
    found.set_verdict(Some(Verdict::Expected));
    let mut out = Decision::default();
    out.writes.push(Write::UpdateIncident(found.clone()));
    out.writes.push(Write::Timeline(IncidentTimelineEntry {
        incident,
        at: now,
        kind: "human.dismissed".to_owned(),
        summary: format!("dismissed by user:{by}: {reason}"),
        detail: json!({ "by": by.to_string(), "reason": reason }),
    }));
    let Some(IncidentChange::StatusChanged { from, to }) = changes.first() else {
        return Ok(out);
    };
    out.writes.push(Write::Timeline(IncidentTimelineEntry {
        incident,
        at: now,
        kind: "status.changed".to_owned(),
        summary: format!("{} → {}", status_name(*from), status_name(*to)),
        detail: json!({ "from": *from, "to": *to }),
    }));
    out.events
        .push(DomainEvent::new(EventKind::IncidentResolved, &found, true));
    Ok(out)
}
