//! Annotating an incident with what triage and actions found: outcomes, verdicts and
//! follow-ups. Pure, like the rest of `decide`.

use std::collections::BTreeMap;

use crate::triage::{CLEAR_KIND, FOLLOW_UP_KIND, ToolUse, count_clear};
use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::{ActionId, Incident, IncidentId, TriageOutcome, UserId, Verdict};
use jiff::Timestamp;

use super::command::{Command, CommandError};
use super::decide::{Decision, State, Write};
use crate::events::{DomainEvent, EventKind};

/// The incident with id `incident`, active or loaded for this command.
fn loaded(state: &State, incident: IncidentId) -> Result<Incident, CommandError> {
    state
        .active
        .iter()
        .chain(state.triaged.iter())
        .find(|i| i.id() == incident)
        .cloned()
        .ok_or(CommandError::UnknownIncident(incident))
}

/// Sets `verdict` on `incident`, for example `Fixed` after a verified action. No event:
/// the action's own event updates the card.
pub(crate) fn set_verdict(
    state: &State,
    incident: IncidentId,
    verdict: Verdict,
    now: Timestamp,
) -> Result<Decision, CommandError> {
    let mut found = loaded(state, incident)?;
    let mut out = Decision::default();
    if found.verdict() == Some(verdict) {
        return Ok(out);
    }
    found.set_verdict(Some(verdict));
    out.writes.push(Write::UpdateIncident(found));
    out.writes.push(Write::Timeline(IncidentTimelineEntry {
        incident,
        at: now,
        kind: "verdict".to_owned(),
        summary: verdict.label().to_owned(),
        detail: serde_json::Value::Null,
    }));
    Ok(out)
}

/// The timeline kind and summary for `outcome`.
fn timeline_line(outcome: &TriageOutcome) -> (&'static str, String) {
    match outcome {
        TriageOutcome::Report(report) => ("triage.report", report.headline.clone()),
        TriageOutcome::Cached { report, from } => (
            "triage.cached",
            format!("{} (reused from {from})", report.headline),
        ),
        TriageOutcome::Skipped { reason } => ("triage.skipped", reason.clone()),
        TriageOutcome::Failed { reason } => ("triage.failed", reason.clone()),
    }
}

/// Stores `outcome` on `incident`: the report's verdict on the incident, a timeline entry,
/// then [`EventKind::TriageCompleted`]. Skipped and failed runs leave the verdict alone.
/// The incident may already be resolved; its status never changes.
pub(crate) fn attach(
    state: &State,
    incident: IncidentId,
    outcome: &TriageOutcome,
    tokens: &BTreeMap<ActionId, String>,
    now: Timestamp,
) -> Result<Decision, CommandError> {
    let mut found = loaded(state, incident)?;
    let mut out = Decision::default();
    if let TriageOutcome::Report(report) | TriageOutcome::Cached { report, .. } = outcome {
        found.set_verdict(Some(report.verdict));
        out.writes.push(Write::UpdateIncident(found.clone()));
    }
    let (kind, summary) = timeline_line(outcome);
    out.writes.push(Write::Timeline(IncidentTimelineEntry {
        incident,
        at: now,
        kind: kind.to_owned(),
        summary,
        detail: serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null),
    }));
    out.events.push(DomainEvent::new(
        EventKind::TriageCompleted {
            outcome: outcome.clone(),
            tokens: tokens.clone(),
        },
        &found,
        true,
    ));
    Ok(out)
}

/// A timeline entry for a follow-up question and its answer. `asked_at` is when the
/// question was asked, before the model ran; the replay order comes from it, not from
/// `now`, so two questions answered out of order still play in question order.
/// Publishes nothing.
pub(crate) fn follow_up(
    incident: IncidentId,
    asked_by: UserId,
    question: &str,
    answer: &str,
    asked_at: Timestamp,
    now: Timestamp,
) -> Decision {
    Decision {
        writes: vec![Write::Timeline(IncidentTimelineEntry {
            incident,
            at: now,
            kind: FOLLOW_UP_KIND.to_owned(),
            summary: question.to_owned(),
            detail: serde_json::json!({
                "asked_by": asked_by.to_string(),
                "question": question,
                "answer": answer,
                "asked_at": asked_at.as_millisecond(),
            }),
        })],
        ..Decision::default()
    }
}

/// The decision for a recorded follow-up. A follow-up request carries the number of
/// clear markers its timeline had when it started; if the live timeline now has more,
/// a clear landed while the model was running and its answer is dropped, so a cleared
/// conversation never gains an older turn.
pub(crate) fn record_follow_up(command: &Command, state: &State, now: Timestamp) -> Decision {
    debug_assert!(matches!(command, Command::RecordFollowUp { .. }));
    match command {
        Command::RecordFollowUp {
            incident,
            asked_by,
            question,
            answer,
            asked_at,
            generation,
        } => {
            if count_clear(&state.timeline) != *generation {
                tracing::info!(
                    %incident,
                    "dropped a follow-up answer: the conversation was cleared while the model was running",
                );
                Decision::default()
            } else {
                follow_up(*incident, *asked_by, question, answer, *asked_at, now)
            }
        }
        _ => Decision::default(),
    }
}

/// A timeline entry that forgets the follow-up questions asked so far, so the next question
/// starts a new conversation. The earlier entries stay in the timeline; only entries before
/// this one are not sent back to the model. Publishes nothing.
pub(crate) fn clear_follow_ups(incident: IncidentId, by: UserId, now: Timestamp) -> Decision {
    Decision {
        writes: vec![Write::Timeline(IncidentTimelineEntry {
            incident,
            at: now,
            kind: CLEAR_KIND.to_owned(),
            summary: format!("cleared by user:{by}"),
            detail: serde_json::Value::Null,
        })],
        ..Decision::default()
    }
}

/// The decision for a command that only appends to an incident's timeline. These need no
/// state; any other command gets an empty decision and is handled by the main match.
/// `RecordFollowUp` is not one of them: it is stale-checkable and has its own function.
pub(crate) fn record(command: &Command, now: Timestamp) -> Decision {
    debug_assert!(matches!(command, Command::RecordToolUse { .. }));
    match command {
        Command::RecordToolUse {
            incident,
            during,
            tools,
        } => tool_use(*incident, during, tools, now),
        _ => Decision::default(),
    }
}

/// A timeline entry for the read tools one run called, with `during` naming the run,
/// `"triage"` or `"follow-up"`. Publishes nothing.
pub(crate) fn tool_use(
    incident: IncidentId,
    during: &str,
    tools: &[ToolUse],
    now: Timestamp,
) -> Decision {
    Decision {
        writes: vec![Write::Timeline(IncidentTimelineEntry {
            incident,
            at: now,
            kind: "triage.tools".to_owned(),
            summary: used_summary(during, tools),
            detail: serde_json::json!({ "during": during, "tools": tools }),
        })],
        ..Decision::default()
    }
}

/// `"{during} used journal, docker_logs ×2"`: display names in call order, with a count
/// only when one repeats. A `run_read_skill` call shows the skill instead.
fn used_summary(during: &str, tools: &[ToolUse]) -> String {
    let mut seen: Vec<(String, u32)> = Vec::new();
    for tool in tools {
        let display = match &tool.skill {
            Some(skill) => skill.to_owned(),
            None => tool.tool.to_owned(),
        };
        if let Some(index) = seen.iter().position(|(name, _)| *name == display) {
            seen[index].1 += 1;
        } else {
            seen.push((display, 1));
        }
    }
    let names = seen
        .iter()
        .map(|(name, count)| {
            if *count > 1 {
                format!("{name} ×{count}")
            } else {
                name.to_owned()
            }
        })
        .collect::<Vec<String>>()
        .join(", ");
    format!("{during} used {names}")
}
