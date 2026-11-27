//! Whether a dismissal still describes an incident, and which known condition it made.

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::triage::is_security_check;
use domovoi_core::{Incident, IncidentStatus};

use super::ApiState;
use super::error::ApiError;

/// The newest known condition made for a dismissed incident: same host and check, and a
/// target pattern equal to the incident's target. A dismissal without a period makes none.
/// Once the incident has reopened, the dismissal no longer describes it: it is named only
/// while its rule still applies, and never for a security event, which is a new one.
pub(super) async fn dismissed_rule(
    state: &ApiState,
    incident: &Incident,
    timeline: &[IncidentTimelineEntry],
) -> Result<Option<String>, ApiError> {
    let Some(dismissed) = timeline
        .iter()
        .rposition(|entry| entry.kind == "human.dismissed")
    else {
        return Ok(None);
    };
    let reopened = timeline[dismissed..].iter().any(|entry| {
        entry.kind == "status.changed"
            && serde_json::from_value::<IncidentStatus>(entry.detail["from"].clone())
                .is_ok_and(|from| from == IncidentStatus::Resolved)
    });
    if reopened && is_security_check(incident.check()) {
        return Ok(None);
    }
    let now = state.deps.clock.now();
    let target = incident.target().map(|t| t.as_str().to_owned());
    Ok(state
        .deps
        .acks
        .list_rules()
        .await?
        .into_iter()
        .filter(|rule| {
            rule.host == Some(incident.host())
                && rule.check == *incident.check()
                && rule.target_pattern.as_ref().map(ToString::to_string) == target
        })
        .max_by_key(|rule| rule.created_at)
        .filter(|rule| !reopened || rule.expires_at.is_none_or(|at| now < at))
        .map(|rule| rule.id.to_string()))
}
