//! Building the card for an incident. Pure.
//!
//! A card works without triage (INV-11): with no report it shows the severity, the
//! incident title and the latest signal's evidence. Destructive actions never get an
//! approve button, only a link to the web UI (INV-6).

use std::collections::BTreeMap;

use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, Card, CardBadge, CardButton, EvidenceValue, HostName,
    Incident, IncidentStatus, Risk, Signal, TriageReport,
};

/// Longest headline, in characters.
pub const HEADLINE_MAX_CHARS: usize = 120;
/// Most bullets on a card.
pub const MAX_BULLETS: usize = domovoi_core::card::MAX_BULLETS;

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

fn action_buttons(actions: &[ActionRequest], links: &CardLinks) -> Vec<CardButton> {
    let mut buttons = Vec::new();
    for action in actions {
        if *action.status() != ActionStatus::Proposed {
            continue;
        }
        let token = links.tokens.get(&action.id());
        match (action.risk(), token) {
            (Risk::Read | Risk::Write, Some(token)) => {
                buttons.push(CardButton::Approve {
                    action: action.id(),
                    token: token.clone(),
                });
                buttons.push(CardButton::Deny {
                    action: action.id(),
                    token: token.clone(),
                });
            }
            // Destructive actions need the web UI's typed confirmation; an action without
            // a chat token cannot be approved from chat either.
            _ => buttons.push(CardButton::OpenInWeb {
                action: action.id(),
            }),
        }
    }
    buttons
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
    buttons.extend(action_buttons(actions, links));
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
        details: details(report),
        resolved,
    }
}
