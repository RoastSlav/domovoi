//! The context bundle sent with the first turn: the incident, its recent signals, what
//! changed, similar incidents, notes and the skills the model may propose. Each part is
//! capped and everything is redacted.

use std::fmt::Write as _;

use domovoi_core::ports::{IncidentFilter, Page, Presence};
use domovoi_core::{Incident, IncidentStatus, ReadToolCall, SkillSummary};
use jiff::SignedDuration;

use super::Shared;
use super::tools::cut;

/// The largest single part of the bundle, in bytes.
pub const PART_BYTES: usize = 4 * 1024;

/// How long fetching what changed on the host may take.
pub const CHANGES_DEADLINE: SignedDuration = SignedDuration::from_secs(2);

const SIGNALS: u16 = 10;
const SIMILAR: usize = 3;
const NOTES: u16 = 3;

/// Builds the redacted bundle for `incident`.
pub(crate) async fn build(shared: &Shared, incident: &Incident, skills: &[SkillSummary]) -> String {
    let parts = [
        ("Incident", snapshot(incident)),
        (
            "Recent signals, newest first",
            signals(shared, incident).await,
        ),
        (
            "What changed in the last 24 hours",
            changes(shared, incident).await,
        ),
        (
            "Similar resolved incidents",
            similar(shared, incident).await,
        ),
        ("Host notes", notes(shared, incident).await),
        ("Skills you may propose", catalogue(skills)),
    ];
    let mut out = String::new();
    for (title, body) in parts {
        let body = if body.trim().is_empty() {
            "None.".to_owned()
        } else {
            cut(body.trim(), PART_BYTES)
        };
        let _ = write!(out, "## {title}\n\n{body}\n\n");
    }
    shared.deps.redactor.redact(out.trim_end())
}

fn snapshot(incident: &Incident) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Title: {}", incident.title());
    let _ = writeln!(out, "Check: {}", incident.check());
    if let Some(target) = incident.target() {
        let _ = writeln!(out, "Target: {target}");
    }
    let _ = writeln!(out, "Host id: {}", incident.host());
    let _ = writeln!(out, "Severity: {}", incident.severity());
    let _ = writeln!(out, "Status: {:?}", incident.status());
    let _ = writeln!(out, "Opened: {}", incident.opened_at());
    let _ = writeln!(out, "Last firing: {}", incident.last_firing_at());
    let _ = writeln!(out, "Firing signals so far: {}", incident.occurrences());
    let history = incident.history();
    if let Some((at, value)) = history.latest() {
        let _ = writeln!(
            out,
            "Tracked value: {value} at {at} ({} samples)",
            history.len()
        );
    }
    out
}

async fn signals(shared: &Shared, incident: &Incident) -> String {
    let Ok(signals) = shared
        .deps
        .signals
        .list_for_incident(incident.id(), SIGNALS)
        .await
    else {
        return String::new();
    };
    let mut out = String::new();
    for signal in signals {
        let evidence: Vec<String> = signal
            .evidence()
            .iter()
            .map(|(k, v)| format!("{k}={}", serde_json::to_string(v).unwrap_or_default()))
            .collect();
        let _ = writeln!(
            out,
            "- {} {:?} [{}] {} {{{}}}",
            signal.observed_at(),
            signal.status(),
            signal.severity(),
            signal.summary(),
            evidence.join(", ")
        );
    }
    out
}

async fn changes(shared: &Shared, incident: &Incident) -> String {
    let deps = &shared.deps;
    let now = deps.clock.now();
    let mut out = String::new();
    let day_ago = now
        .checked_sub(SignedDuration::from_hours(24))
        .unwrap_or(now);
    let filter = IncidentFilter {
        host: Some(incident.host()),
        opened_since: Some(day_ago),
        ..IncidentFilter::default()
    };
    if let Ok(recent) = deps.incidents.list(&filter, &Page::first(50)).await {
        for reboot in recent
            .items
            .iter()
            .filter(|i| i.check().as_str() == "system.reboot")
        {
            let _ = writeln!(out, "- The host rebooted around {}.", reboot.opened_at());
        }
    }
    if matches!(
        deps.link.presence(incident.host()),
        Presence::Connected { .. }
    ) {
        let deadline = now.checked_add(CHANGES_DEADLINE).unwrap_or(now);
        let call = ReadToolCall::DockerEvents {
            container: None,
            since_minutes: 24 * 60,
        };
        match deps.link.run_tool(incident.host(), call, deadline).await {
            Ok(events) if !events.text.trim().is_empty() => {
                let _ = writeln!(out, "Docker events:\n{}", events.text.trim());
            }
            Ok(_) => {}
            Err(err) => tracing::debug!(%err, "docker events skipped in the triage context"),
        }
    }
    out
}

async fn similar(shared: &Shared, incident: &Incident) -> String {
    let query = incident
        .target()
        .map_or_else(|| incident.check().as_str(), |t| t.as_str());
    let Ok(found) = shared.deps.incidents.search(query, 20).await else {
        return String::new();
    };
    found
        .iter()
        .filter(|i| {
            i.id() != incident.id()
                && i.status() == IncidentStatus::Resolved
                && i.check() == incident.check()
        })
        .take(SIMILAR)
        .map(|i| {
            let verdict = i.verdict().map_or("no verdict", |v| v.label());
            format!(
                "- {} {}: {} ({verdict})",
                i.opened_at(),
                i.check(),
                i.title()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn notes(shared: &Shared, incident: &Incident) -> String {
    let mut query = incident.check().as_str().replace(['.', '_'], " ");
    if let Some(target) = incident.target() {
        query.push(' ');
        query.push_str(target.as_str());
    }
    let Ok(hits) = shared
        .deps
        .notes
        .search(&query, Some(incident.host()), NOTES)
        .await
    else {
        return String::new();
    };
    hits.iter()
        .map(|h| format!("- {}: {}", h.note.title, h.snippet))
        .collect::<Vec<_>>()
        .join("\n")
}

fn catalogue(skills: &[SkillSummary]) -> String {
    let mut out = String::new();
    for skill in skills {
        let params = serde_json::to_string(&skill.params).unwrap_or_default();
        let applies: Vec<&str> = skill.applies_to.iter().map(|c| c.as_str()).collect();
        let _ = writeln!(
            out,
            "- {} (risk: {:?}): {} Parameters: {params}.{}",
            skill.name,
            skill.risk,
            skill.description.trim(),
            if applies.is_empty() {
                String::new()
            } else {
                format!(" Only for checks: {}.", applies.join(", "))
            }
        );
    }
    out
}
