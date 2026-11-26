//! Building the card for an incident. Pure.
//!
//! A card works without triage (INV-11): with no report it shows the severity, the
//! incident title and the latest signal's evidence. At most three waiting steps are
//! listed oldest first, with what each will do. Destructive steps, and steps whose
//! arguments are too long to show, never get an approve button; they get a link to the
//! web UI instead (INV-6).

use std::collections::BTreeMap;

use domovoi_core::{
    Card, CardBadge, CardButton, CardPlan, EvidenceValue, ExecutionId, HostName, Incident,
    IncidentStatus, Risk, Signal, TriageReport,
};
use jiff::Timestamp;

/// Longest headline, in characters.
pub const HEADLINE_MAX_CHARS: usize = 120;
/// Most bullets on a card.
pub const MAX_BULLETS: usize = domovoi_core::card::MAX_BULLETS;
/// Most waiting steps a card lists and offers buttons for.
pub const MAX_PLANS: usize = domovoi_core::card::MAX_PLANS;

/// What the card needs from outside the incident.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardLinks {
    /// The web UI's public URL, for links.
    pub web_url: Option<String>,
    /// The single-use chat token minted for each waiting step, keyed by its execution. The
    /// store keeps only its hash, so the caller that minted it passes it here. A step without
    /// a token gets a web link instead of approve and deny buttons.
    pub tokens: BTreeMap<ExecutionId, String>,
    /// Execution steps waiting for approval.
    pub steps: Vec<WaitingStep>,
}

/// One Execution step waiting for approval, as the card needs it. Built from the stored
/// record only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingStep {
    /// The execution, which is also the step id.
    pub execution: ExecutionId,
    /// The skill name.
    pub skill: String,
    /// The host it runs on.
    pub host: HostName,
    /// The risk stored when the step was proposed; `None` when unknown.
    pub risk: Option<Risk>,
    /// The arguments, sorted by name.
    pub args: Vec<(String, String)>,
    /// The first characters of the v2 plan hash, for display.
    pub plan_short: String,
    /// The first 3 bytes of the v2 plan hash as 6 hex characters, for the approve button.
    pub plan_prefix: String,
    /// When the approval window closes.
    pub expires_at: Timestamp,
    /// When the step was proposed.
    pub created_at: Timestamp,
}

/// What a step will do, from the stored record. An unknown risk is shown as the most
/// dangerous one.
fn step_plan(step: &WaitingStep) -> CardPlan {
    CardPlan {
        execution: step.execution,
        skill: step.skill.clone(),
        host: step.host.clone(),
        risk: step.risk.unwrap_or(Risk::Destructive),
        args: step.args.clone(),
        plan_short: step.plan_short.clone(),
        expires_at: step.expires_at,
    }
}

/// Whether a step may carry chat buttons: its risk is read or write and its arguments fit on
/// the card. Everything else is approved in the web UI (INV-6).
#[must_use]
pub fn step_offers_chat(step: &WaitingStep) -> bool {
    matches!(step.risk, Some(Risk::Read | Risk::Write)) && step_plan(step).args_text().is_some()
}

/// `text` cut to at most `max` characters, ending in "…" when cut.
#[must_use]
pub fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// An evidence value as shown on a card.
#[must_use]
pub fn evidence_text(value: &EvidenceValue) -> String {
    match value {
        EvidenceValue::Text(text) => text.clone(),
        EvidenceValue::Int(n) => n.to_string(),
        EvidenceValue::Float(f) => {
            let rounded = (f * 100.0).round() / 100.0;
            rounded.to_string()
        }
        EvidenceValue::Bool(b) => b.to_string(),
    }
}

fn bullets(report: Option<&TriageReport>, latest: Option<&Signal>) -> Vec<String> {
    match (report, latest) {
        (Some(report), _) if !report.evidence.is_empty() => report
            .evidence
            .iter()
            .take(MAX_BULLETS)
            .map(|e| clip(&format!("{}: {}", e.label, e.value), HEADLINE_MAX_CHARS))
            .collect(),
        (_, Some(signal)) => signal
            .evidence()
            .iter()
            .take(MAX_BULLETS)
            .map(|(key, value)| {
                clip(
                    &format!("{key}: {}", evidence_text(value)),
                    HEADLINE_MAX_CHARS,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The waiting steps a card may show, oldest first, at most [`MAX_PLANS`]. The store returns
/// newest first, so the order is restored from the creation time and the id.
fn waiting(steps: &[WaitingStep]) -> Vec<&WaitingStep> {
    let mut all: Vec<&WaitingStep> = steps.iter().collect();
    all.sort_by_key(|step| (step.created_at, step.execution));
    all.truncate(MAX_PLANS);
    all
}

/// The plan lines and buttons for the waiting `links.steps`.
///
/// Approve and deny carry the first hex characters of the plan hash so a press is bound to
/// the plan it showed. A step whose arguments would not fit on the card, a destructive or
/// unknown-risk one and one without a chat token get a web link instead, where every
/// argument is visible.
fn waiting_plans_and_buttons(links: &CardLinks) -> (Vec<CardPlan>, Vec<CardButton>) {
    let mut plans = Vec::new();
    let mut buttons = Vec::new();
    for step in waiting(&links.steps) {
        let plan = step_plan(step);
        match links.tokens.get(&plan.execution) {
            Some(token) if step_offers_chat(step) => {
                buttons.push(CardButton::Approve {
                    execution: plan.execution,
                    token: token.clone(),
                    plan_prefix: Some(step.plan_prefix.clone()),
                });
                buttons.push(CardButton::Deny {
                    execution: plan.execution,
                    token: token.clone(),
                });
            }
            _ => buttons.push(CardButton::OpenInWeb {
                execution: plan.execution,
            }),
        }
        plans.push(plan);
    }
    (plans, buttons)
}

fn details(report: Option<&TriageReport>) -> Option<String> {
    let report = report?;
    let parts: Vec<&str> = [
        report.summary.as_str(),
        report.cause.as_str(),
        report.details.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .filter(|p| !p.is_empty())
    .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// The card for `incident` on `host`, with the triage `report` if there is one, buttons for
/// its waiting steps, and evidence from the `latest` signal when there is no report.
#[must_use]
pub fn build_card(
    incident: &Incident,
    report: Option<&TriageReport>,
    host: &HostName,
    links: &CardLinks,
    latest: Option<&Signal>,
) -> Card {
    let resolved = incident.status() == IncidentStatus::Resolved;
    let badge = match report.map(|r| r.verdict).or(incident.verdict()) {
        Some(verdict) => CardBadge::Verdict(verdict),
        None => CardBadge::Severity(incident.severity()),
    };
    let headline = clip(
        report.map_or(incident.title(), |r| r.headline.as_str()),
        HEADLINE_MAX_CHARS,
    );
    let status = match incident.status() {
        IncidentStatus::Acked => Some("Acknowledged".to_owned()),
        IncidentStatus::Muted { until } => {
            Some(format!("Muted until {}", until.strftime("%H:%M UTC")))
        }
        _ => None,
    };
    let mut buttons = Vec::new();
    if !resolved {
        // A button for what was already done would only answer "not possible".
        if status.is_none() {
            buttons.push(CardButton::Ack);
        }
        if !matches!(incident.status(), IncidentStatus::Muted { .. }) {
            buttons.push(CardButton::Mute1h);
        }
    }
    buttons.push(CardButton::Details);
    let (plans, waiting_buttons) = waiting_plans_and_buttons(links);
    buttons.extend(waiting_buttons);
    Card {
        incident: incident.id(),
        host: host.clone(),
        badge,
        headline,
        bullets: bullets(report, latest),
        next_step: report
            .map(|r| r.next_step.trim().to_owned())
            .filter(|s| !s.is_empty()),
        buttons,
        plans,
        details: details(report),
        resolved,
        status,
    }
}
