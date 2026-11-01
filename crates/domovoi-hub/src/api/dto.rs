//! What the API returns for incidents, reports and actions. Core types are mapped here and
//! never exposed directly. Ids are strings and times RFC 3339.

use std::collections::BTreeMap;

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::{ActionRequest, ActionStatus, Card, Incident, Principal, TriageReport};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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
            verdict: i.verdict().map(|v| v.label().to_owned()),
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
}

/// Filters for the incident list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct IncidentQuery {
    /// Only this status: `pending`, `open`, `acked`, `muted` or `resolved`.
    pub status: Option<String>,
    /// Only this host, by name.
    pub host: Option<String>,
    /// Only this severity: `info`, `warning` or `critical`.
    pub severity: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 200. Default 50.
    pub limit: Option<u16>,
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
    /// Suggested actions.
    pub proposals: Vec<ProposalDto>,
    /// Anything else.
    pub details: String,
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
            verdict: r.verdict.label().to_owned(),
            next_step: r.next_step.clone(),
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

/// The incident's chat card, as it would be posted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CardDto {
    /// The badge text: a verdict or a severity.
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
        let badge = serde_json::to_value(c.badge)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
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
    /// Actions for this incident, newest first.
    pub actions: Vec<ActionDto>,
}

/// An action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ActionDto {
    /// Action id.
    pub id: String,
    /// The incident it is for.
    pub incident_id: String,
    /// The host it runs on.
    pub host_id: String,
    /// Skill name.
    pub skill: String,
    /// `read`, `write` or `destructive`.
    pub risk: String,
    /// Arguments by parameter name.
    pub args: BTreeMap<String, String>,
    /// The status name, for example `proposed` or `verified`.
    pub status: String,
    /// Why it was rejected or failed.
    pub reason: Option<String>,
    /// Who proposed it, for example `system:triage`.
    pub proposed_by: String,
    /// Who approved it, if anyone.
    pub approved_by: Option<String>,
    /// When the approval window closes.
    pub expires_at: String,
    /// When it was proposed.
    pub created_at: String,
    /// When it last changed.
    pub updated_at: String,
}

impl From<&ActionRequest> for ActionDto {
    fn from(a: &ActionRequest) -> Self {
        let reason = match a.status() {
            ActionStatus::Rejected { reason } | ActionStatus::Failed { reason } => {
                Some(reason.clone())
            }
            _ => None,
        };
        let risk = serde_json::to_value(a.risk())
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        Self {
            id: a.id().to_string(),
            incident_id: a.incident().to_string(),
            host_id: a.host().to_string(),
            skill: a.skill().to_string(),
            risk,
            args: a.args().clone(),
            status: a.status().name().to_owned(),
            reason,
            proposed_by: match a.proposed_by() {
                Principal::Human(user) => format!("user:{user}"),
                Principal::System(component) => format!("system:{component}"),
            },
            approved_by: a.approved_by().map(|h| format!("user:{}", h.user())),
            expires_at: a.expires_at().to_string(),
            created_at: a.created_at().to_string(),
            updated_at: a.updated_at().to_string(),
        }
    }
}

/// One page of actions, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ActionPage {
    /// The actions.
    pub items: Vec<ActionDto>,
    /// Pass as `cursor` for the next page; absent on the last.
    pub next_cursor: Option<String>,
}

/// Filters for the action list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActionQuery {
    /// Only this status name, for example `proposed`.
    pub status: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 200. Default 50.
    pub limit: Option<u16>,
}

/// Approving an action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ApproveRequest {
    /// For destructive skills: the skill's name typed exactly, as confirmation.
    pub confirm_text: Option<String>,
}

/// Muting an incident.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct MuteRequest {
    /// How long, in minutes, 1 to 10080 (a week).
    pub minutes: u32,
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
    /// Plain text, at most 1500 characters.
    pub answer: String,
}
