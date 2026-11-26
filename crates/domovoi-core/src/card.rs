//! Cards: the platform-independent shape of every incident message (ADR 0010).

use crate::error::ValidationError;
use crate::host::HostName;
use crate::id::{ExecutionId, IncidentId};
use crate::severity::Severity;
use crate::skill::Risk;
use crate::verdict::Verdict;
use jiff::Timestamp;

/// Most bullets on one card.
pub const MAX_BULLETS: usize = 3;

/// Most waiting actions one card shows.
pub const MAX_PLANS: usize = 3;

/// What one waiting action will do, built from the stored action (never from model text).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CardPlan {
    /// The execution the plan describes.
    pub execution: ExecutionId,
    /// Which skill runs.
    pub skill: String,
    /// Where it runs.
    pub host: HostName,
    /// What running it can do.
    pub risk: Risk,
    /// Validated arguments, in name order.
    pub args: Vec<(String, String)>,
    /// The first 8 hex digits of the action's approval-family plan hash.
    pub plan_short: String,
    /// When the approval window closes.
    pub expires_at: Timestamp,
}

impl CardPlan {
    /// Longest [`CardPlan::args_text`], in characters.
    pub const ARGS_BUDGET: usize = 500;

    /// `name=value, name=value`, in name order, or `None` when it is longer than
    /// [`Self::ARGS_BUDGET`]. Newlines and control characters in values become single
    /// spaces; backticks become `'`. "no arguments" when there are none.
    #[must_use]
    pub fn args_text(&self) -> Option<String> {
        if self.args.is_empty() {
            return Some("no arguments".to_owned());
        }
        let mut text = String::new();
        for (name, value) in &self.args {
            if !text.is_empty() {
                text.push_str(", ");
            }
            text.push_str(name);
            text.push('=');
            for ch in value.chars() {
                text.push(match ch {
                    '`' => '\'',
                    c if c.is_control() => ' ',
                    c => c,
                });
            }
        }
        (text.chars().count() <= Self::ARGS_BUDGET).then_some(text)
    }
}

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
    /// Approve a proposed execution. Only allowlisted users' presses are honoured.
    Approve {
        /// The action.
        execution: ExecutionId,
        /// Single-use token that expires with the approval window.
        token: String,
        /// The first 6 lowercase hex characters of the plan hash, when the button carried
        /// one. A card from before the upgrade has none and cannot approve.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_prefix: Option<String>,
    },
    /// Deny a proposed execution.
    Deny {
        /// The action.
        execution: ExecutionId,
        /// Single-use token that expires with the approval window.
        token: String,
    },
    /// Link to the web UI, used for destructive actions that cannot be approved in chat.
    OpenInWeb {
        /// The execution.
        execution: ExecutionId,
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
    /// Up to [`MAX_PLANS`] waiting actions and what each will do.
    #[serde(default)]
    pub plans: Vec<CardPlan>,
    /// Full reasoning, posted in the thread.
    pub details: Option<String>,
    /// Whether the incident has resolved.
    pub resolved: bool,
}

impl Card {
    /// Checks the bullet and plan limits.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.bullets.len() > MAX_BULLETS {
            return Err(ValidationError::new(
                "bullets",
                format!("must have at most {MAX_BULLETS} items"),
            ));
        }
        if self.plans.len() > MAX_PLANS {
            return Err(ValidationError::new(
                "plans",
                format!("must have at most {MAX_PLANS} items"),
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
            plans: Vec::new(),
            details: None,
            resolved: false,
        };
        assert!(card.validate().is_ok());
        card.bullets.push("d".into());
        assert!(card.validate().is_err());
    }

    fn plan(args: Vec<(&str, &str)>) -> CardPlan {
        CardPlan {
            execution: ExecutionId::new(),
            skill: "restart_container".to_owned(),
            host: HostName::parse("nas").unwrap(),
            risk: Risk::Write,
            args: args
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            plan_short: "0123abcd".to_owned(),
            expires_at: "2026-10-31T13:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn card_rejects_more_than_three_plans() {
        let mut card = Card {
            incident: IncidentId::new(),
            host: HostName::parse("nas").unwrap(),
            badge: CardBadge::Severity(Severity::Warning),
            headline: "disk almost full".to_owned(),
            bullets: Vec::new(),
            next_step: None,
            buttons: Vec::new(),
            plans: vec![plan(Vec::new()), plan(Vec::new()), plan(Vec::new())],
            details: None,
            resolved: false,
        };
        assert!(card.validate().is_ok());
        card.plans.push(plan(Vec::new()));
        assert!(card.validate().is_err());
    }

    #[test]
    fn card_plan_args_text_sanitises_and_respects_the_budget() {
        let sanitised = plan(vec![("container", "plex\n`rm -rf`")]);
        assert_eq!(
            sanitised.args_text().as_deref(),
            Some("container=plex 'rm -rf'")
        );

        let empty = plan(Vec::new());
        assert_eq!(empty.args_text().as_deref(), Some("no arguments"));

        let exactly_value = "v".repeat(CardPlan::ARGS_BUDGET - 2);
        let exactly = plan(vec![("k", exactly_value.as_str())]);
        assert_eq!(
            exactly.args_text().map(|text| text.chars().count()),
            Some(CardPlan::ARGS_BUDGET)
        );

        let over_value = "v".repeat(CardPlan::ARGS_BUDGET - 1);
        let over = plan(vec![("k", over_value.as_str())]);
        assert_eq!(over.args_text(), None);
    }
}
