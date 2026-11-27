//! What the API returns for incidents, reports and actions. Core types are mapped here and
//! never exposed directly. Ids are strings and times RFC 3339.

use std::collections::BTreeMap;

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::{Card, CardBadge, Incident, TriageReport, Verdict};
use domovoi_engine::triage::Turn;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use super::dto_query::{IncidentQuery, LogsQuery, RetryQuery};

/// The wire name of `verdict`, for example `ACTION_NEEDED`.
fn verdict_name(verdict: Verdict) -> String {
    serde_json::to_value(verdict)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// One incident in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct IncidentSummary {
    /// Incident id.
    pub id: String,
    /// Host id.
    pub host_id: String,
    /// What was checked, for example `disk.usage`.
    pub check: String,
    /// What it was checked on, for example `/srv`.
    pub target: Option<String>,
    /// One line for people.
    pub title: String,
    /// `pending`, `open`, `acked`, `muted` or `resolved`.
    pub status: String,
    /// When a mute ends, if muted.
    pub muted_until: Option<String>,
    /// `info`, `warning` or `critical`.
    pub severity: String,
    /// Triage's verdict, if any: `ACTION_NEEDED`, `MONITOR`, `EXPECTED` or `FIXED`.
    pub verdict: Option<String>,
    /// When the first signal arrived.
    pub opened_at: String,
    /// When it last fired.
    pub last_firing_at: String,
    /// When it resolved.
    pub resolved_at: Option<String>,
    /// Firing signals folded into it.
    pub occurrences: u32,
}

impl From<&Incident> for IncidentSummary {
    fn from(i: &Incident) -> Self {
        let muted_until = match i.status() {
            domovoi_core::IncidentStatus::Muted { until } => Some(until.to_string()),
            _ => None,
        };
        Self {
            id: i.id().to_string(),
            host_id: i.host().to_string(),
            check: i.check().to_string(),
            target: i.target().map(ToString::to_string),
            title: i.title().to_owned(),
            status: i.status().name().to_owned(),
            muted_until,
            severity: i.severity().to_string(),
            verdict: i.verdict().map(verdict_name),
            opened_at: i.opened_at().to_string(),
            last_firing_at: i.last_firing_at().to_string(),
            resolved_at: i.resolved_at().map(|t| t.to_string()),
            occurrences: i.occurrences(),
        }
    }
}

/// One page of incidents, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct IncidentPage {
    /// The incidents.
    pub items: Vec<IncidentSummary>,
    /// Pass as `cursor` for the next page; absent on the last.
    pub next_cursor: Option<String>,
    /// The journal position this snapshot was read at; apply only events with a greater `seq`.
    pub as_of_seq: u64,
}

/// One captured log line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LogLineDto {
    /// When the line was logged, RFC 3339.
    pub at: String,
    /// How severe the line is: `error`, `warn`, `info`, `debug` or `trace`.
    pub level: String,
    /// What logged the line, usually the module that called it.
    pub target: String,
    /// The message with its fields as `key=value` pairs, secrets removed.
    pub message: String,
}

/// One piece of evidence in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct EvidenceDto {
    /// What it shows.
    pub label: String,
    /// The value.
    pub value: String,
    /// Where it came from.
    pub source: String,
}

/// An action triage suggested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProposalDto {
    /// Skill name.
    pub skill: String,
    /// Arguments by parameter name.
    pub args: BTreeMap<String, String>,
    /// Why.
    pub reason: String,
}

/// A triage report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReportDto {
    /// What is wrong, in one line.
    pub headline: String,
    /// What happened.
    pub summary: String,
    /// The likely cause.
    pub cause: String,
    /// Supporting evidence.
    pub evidence: Vec<EvidenceDto>,
    /// `ACTION_NEEDED`, `MONITOR`, `EXPECTED` or `FIXED`.
    pub verdict: String,
    /// The single next step.
    pub next_step: String,
    /// Steps a person does by hand, in order.
    pub manual_steps: Vec<String>,
    /// Suggested actions.
    pub proposals: Vec<ProposalDto>,
    /// Anything else.
    pub details: String,
    /// Fields that repeat an earlier one or break their job (a command in the next step),
    /// found when the report was accepted. The page shows a repeated field once.
    pub field_problems: Vec<String>,
}

impl From<&TriageReport> for ReportDto {
    fn from(r: &TriageReport) -> Self {
        Self {
            headline: r.headline.clone(),
            summary: r.summary.clone(),
            cause: r.cause.clone(),
            evidence: r
                .evidence
                .iter()
                .map(|e| EvidenceDto {
                    label: e.label.clone(),
                    value: e.value.clone(),
                    source: e.source.clone(),
                })
                .collect(),
            verdict: verdict_name(r.verdict),
            next_step: r.next_step.clone(),
            manual_steps: r.manual_steps.clone(),
            proposals: r
                .proposals
                .iter()
                .map(|p| ProposalDto {
                    skill: p.skill.to_string(),
                    args: p.args.clone(),
                    reason: p.reason.clone(),
                })
                .collect(),
            details: r.details.clone(),
            field_problems: r.field_problems().into_iter().map(str::to_owned).collect(),
        }
    }
}

/// How triage went for an incident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TriageStateDto {
    /// `report`, `cached`, `skipped` or `failed`.
    pub kind: String,
    /// Why it was skipped or failed.
    pub reason: Option<String>,
}

impl From<&domovoi_core::TriageOutcome> for TriageStateDto {
    /// How a triage run went, as the API shows it.
    fn from(outcome: &domovoi_core::TriageOutcome) -> Self {
        use domovoi_core::TriageOutcome;
        let (kind, reason) = match outcome {
            TriageOutcome::Report(_) => ("report", None),
            TriageOutcome::Cached { .. } => ("cached", None),
            TriageOutcome::Skipped { reason } => ("skipped", Some(reason.clone())),
            TriageOutcome::Failed { reason } => ("failed", Some(reason.clone())),
        };
        Self {
            kind: kind.to_owned(),
            reason,
        }
    }
}

/// The incident's chat card, as it would be posted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CardDto {
    /// A verdict (`ACTION_NEEDED`, `MONITOR`, `EXPECTED` or `FIXED`) or, before triage, a
    /// severity (`info`, `warning` or `critical`).
    pub badge: String,
    /// One line.
    pub headline: String,
    /// Key facts.
    pub bullets: Vec<String>,
    /// The next step, if triage gave one.
    pub next_step: Option<String>,
    /// Summary, cause and details, if triage ran.
    pub details: Option<String>,
    /// Whether the incident resolved.
    pub resolved: bool,
}

impl From<&Card> for CardDto {
    fn from(c: &Card) -> Self {
        let badge = match c.badge {
            CardBadge::Verdict(v) => verdict_name(v),
            CardBadge::Severity(s) => s.to_string(),
        };
        Self {
            badge,
            headline: c.headline.clone(),
            bullets: c.bullets.clone(),
            next_step: c.next_step.clone(),
            details: c.details.clone(),
            resolved: c.resolved,
        }
    }
}

/// One timeline line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TimelineEntryDto {
    /// When.
    pub at: String,
    /// What kind, for example `incident.opened` or `triage.report`.
    pub kind: String,
    /// One line.
    pub summary: String,
}

impl From<&IncidentTimelineEntry> for TimelineEntryDto {
    fn from(e: &IncidentTimelineEntry) -> Self {
        Self {
            at: e.at.to_string(),
            kind: e.kind.clone(),
            summary: e.summary.clone(),
        }
    }
}

/// Everything about one incident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct IncidentDetail {
    /// The incident.
    pub incident: IncidentSummary,
    /// The host's name.
    pub host: String,
    /// The latest triage report, if any.
    pub report: Option<ReportDto>,
    /// How the latest triage went, if it ran.
    pub triage: Option<TriageStateDto>,
    /// The chat card.
    pub card: CardDto,
    /// The timeline, oldest first.
    pub timeline: Vec<TimelineEntryDto>,
    /// Operations about this incident, newest first.
    pub operations: Vec<super::dto_operations::OperationSummaryDto>,
    /// Skills and tools used while triaging or answering, in first-call order.
    pub tools_used: Vec<ToolUsageDto>,
    /// The journal position this snapshot was read at; apply only events with a greater `seq`.
    pub as_of_seq: u64,
    /// The id of the known condition made when this incident was dismissed with a period, so
    /// the page can link to it; null when it was not dismissed or no rule was made.
    pub known_condition: Option<String>,
}

/// One skill or tool used while triaging or answering a follow-up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ToolUsageDto {
    /// The tool name, or the skill name for a read skill.
    pub name: String,
    /// `tool` or `skill`.
    pub kind: String,
    /// How many times it was called.
    pub calls: u32,
    /// How many of those calls failed.
    pub failures: u32,
    /// How many of those calls were not run because the tool-call budget was spent.
    pub refused: u32,
    /// When it was last called.
    pub last_used: String,
}

/// Muting an incident.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct MuteRequest {
    /// How long, in minutes, 1 to 10080 (a week).
    pub minutes: u32,
}

/// Dismissing an incident as a known condition.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct DismissRequest {
    /// Why this is not a real problem, 1 to 500 characters.
    pub reason: String,
    /// Minutes to keep the check quiet, 1 to 525600; 0 means always; null dismisses
    /// without creating a rule, so the check alerts again.
    pub ignore_for_minutes: Option<u32>,
}

/// A follow-up question.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct AskRequest {
    /// The question, at most 1000 characters.
    pub question: String,
}

/// The answer to a follow-up question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AskResponse {
    /// Plain text, at most 4000 characters.
    pub answer: String,
}

/// One question and its answer of the conversation the model still remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TurnDto {
    /// What the operator asked.
    pub question: String,
    /// What the model answered.
    pub answer: String,
    /// Who asked, as `user:` and its user id.
    pub asked_by: String,
    /// When the answer was recorded, RFC 3339.
    pub at: String,
}

/// The follow-up conversation the model still remembers for an incident, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct FollowUpsDto {
    /// The remembered questions and answers; empty when there are none or after a clear.
    pub turns: Vec<TurnDto>,
}

impl From<&Turn> for TurnDto {
    fn from(t: &Turn) -> Self {
        Self {
            question: t.question.clone(),
            answer: t.answer.clone(),
            asked_by: format!("user:{}", t.asked_by),
            at: t.at.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "dto_tests.rs"]
mod tests;
