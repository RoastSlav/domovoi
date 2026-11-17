//! Building the card for an incident. Pure.
//!
//! A card works without triage (INV-11): with no report it shows the severity, the
//! incident title and the latest signal's evidence. At most three waiting actions are
//! listed oldest first, with what each will do. Destructive actions, and actions whose
//! arguments are too long to show, never get an approve button; they get a link to the
//! web UI instead (INV-6).

use std::collections::BTreeMap;

use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, Card, CardBadge, CardButton, CardPlan, EvidenceValue,
    HostName, Incident, IncidentStatus, Risk, Signal, TriageReport,
};

/// Longest headline, in characters.
pub const HEADLINE_MAX_CHARS: usize = 120;
/// Most bullets on a card.
pub const MAX_BULLETS: usize = domovoi_core::card::MAX_BULLETS;
/// Most waiting actions a card lists and offers buttons for.
pub const MAX_PLANS: usize = domovoi_core::card::MAX_PLANS;
/// Hex characters of the plan hash shown on a chat approve button, from its first 3 bytes.
const PLAN_PREFIX_CHARS: usize = 6;

/// What the card needs from outside the incident.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardLinks {
    /// The web UI's public URL, for links.
    pub web_url: Option<String>,
    /// The single-use chat token minted for each proposed action. The store keeps only
    /// its hash, so the caller that minted it passes it here. An action without a token
    /// gets a web link instead of approve and deny buttons.
    pub tokens: BTreeMap<ActionId, String>,
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

/// The proposed actions a card may show, oldest first, at most [`MAX_PLANS`]. The store
/// returns newest first, so the order is restored from the creation time and the id.
fn waiting(actions: &[ActionRequest]) -> Vec<&ActionRequest> {
    let mut proposed: Vec<&ActionRequest> = actions
        .iter()
        .filter(|action| *action.status() == ActionStatus::Proposed)
        .collect();
    proposed.sort_by(|a, b| {
        a.created_at()
            .cmp(&b.created_at())
            .then_with(|| a.id().cmp(&b.id()))
    });
    proposed.truncate(MAX_PLANS);
    proposed
}

/// What `action` will do, from the stored request rather than model text.
fn plan_for(action: &ActionRequest, host: &HostName) -> CardPlan {
    CardPlan {
        action: action.id(),
        skill: action.skill().to_string(),
        host: host.clone(),
        risk: action.risk(),
        args: action
            .args()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        plan_short: action.plan_hash_for_approval().short(),
        expires_at: action.expires_at(),
    }
}

/// The plan lines and buttons for the waiting `actions`.
///
/// Approve and deny carry the first [`PLAN_PREFIX_CHARS`] hex characters of the plan hash
/// so a press is bound to the plan it showed. An action whose arguments would not fit on
/// the card, a destructive action and an action without a chat token get a web link
/// instead, where every argument is visible.
fn waiting_plans_and_buttons(
    actions: &[ActionRequest],
    host: &HostName,
    links: &CardLinks,
) -> (Vec<CardPlan>, Vec<CardButton>) {
    let mut plans = Vec::new();
    let mut buttons = Vec::new();
    for action in waiting(actions) {
        let plan = plan_for(action, host);
        let token = links.tokens.get(&action.id());
        match (plan.risk, token) {
            (Risk::Read | Risk::Write, Some(token)) if plan.args_text().is_some() => {
                let prefix: String = hex::encode(&action.plan_hash_for_approval().as_bytes()[..3])
                    .chars()
                    .take(PLAN_PREFIX_CHARS)
                    .collect();
                buttons.push(CardButton::Approve {
                    action: plan.action,
                    token: token.clone(),
                    plan_prefix: Some(prefix),
                });
                buttons.push(CardButton::Deny {
                    action: plan.action,
                    token: token.clone(),
                });
            }
            _ => buttons.push(CardButton::OpenInWeb {
                action: plan.action,
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
/// its proposed `actions`, and evidence from the `latest` signal when there is no report.
#[must_use]
pub fn build_card(
    incident: &Incident,
    report: Option<&TriageReport>,
    actions: &[ActionRequest],
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
    let mut buttons = Vec::new();
    if !resolved {
        buttons.push(CardButton::Ack);
        buttons.push(CardButton::Mute1h);
    }
    buttons.push(CardButton::Details);
    let (plans, waiting_buttons) = waiting_plans_and_buttons(actions, host, links);
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
    }
}
