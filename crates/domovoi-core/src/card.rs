//! Cards: the platform-independent shape of every incident message (ADR 0010).

use crate::error::ValidationError;
use crate::host::HostName;
use crate::id::{ActionId, IncidentId};
use crate::severity::Severity;
use crate::verdict::Verdict;

/// Most bullets on one card.
pub const MAX_BULLETS: usize = 3;

/// The label in the card's corner: the triage verdict, or the severity if there is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardBadge {
    /// Triage has a verdict.
    Verdict(Verdict),
    /// No verdict yet; show the severity.
    Severity(Severity),
}

/// A button on a card. Gateways map these to platform controls.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CardButton {
    /// Acknowledge the incident.
    Ack,
    /// Mute the incident for one hour.
    Mute1h,
    /// Post the full details in the thread.
    Details,
    /// Approve a proposed action. Only allowlisted users' presses are honoured.
    Approve {
        /// The action.
        action: ActionId,
        /// Single-use token that expires with the action.
        token: String,
    },
    /// Deny a proposed action.
    Deny {
        /// The action.
        action: ActionId,
        /// Single-use token that expires with the action.
        token: String,
    },
    /// Link to the web UI, used for destructive actions that cannot be approved in chat.
    OpenInWeb {
        /// The action.
        action: ActionId,
    },
}

/// One incident message, independent of any chat platform.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Card {
    /// The incident shown.
    pub incident: IncidentId,
    /// The affected host.
    pub host: HostName,
    /// Verdict or severity label.
    pub badge: CardBadge,
    /// One-line summary.
    pub headline: String,
    /// Up to [`MAX_BULLETS`] short facts.
    pub bullets: Vec<String>,
    /// What to do next.
    pub next_step: Option<String>,
    /// Controls.
    pub buttons: Vec<CardButton>,
    /// Full reasoning, posted in the thread.
    pub details: Option<String>,
    /// Whether the incident has resolved.
    pub resolved: bool,
}

impl Card {
    /// Checks the bullet limit.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.bullets.len() > MAX_BULLETS {
            return Err(ValidationError::new(
                "bullets",
                format!("must have at most {MAX_BULLETS} items"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_rejects_more_than_three_bullets() {
        let mut card = Card {
            incident: IncidentId::new(),
            host: HostName::parse("nas").unwrap(),
            badge: CardBadge::Severity(Severity::Warning),
            headline: "disk almost full".to_owned(),
            bullets: vec!["a".into(), "b".into(), "c".into()],
            next_step: None,
            buttons: vec![CardButton::Ack],
            details: None,
            resolved: false,
        };
        assert!(card.validate().is_ok());
        card.bullets.push("d".into());
        assert!(card.validate().is_err());
    }
}
