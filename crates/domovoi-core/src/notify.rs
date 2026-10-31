//! What has been said about an incident, so the notification policy knows what to say next.

use std::collections::BTreeMap;

use jiff::Timestamp;

use crate::ports::{MessageRef, ThreadRef};
use crate::severity::Severity;

/// Where an incident's card and thread are on one chat platform.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChatRefs {
    /// The posted card.
    pub message: MessageRef,
    /// The thread under it, once opened.
    pub thread: Option<ThreadRef>,
}

/// An alert held back during quiet hours, released with the others when they end.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeldItem {
    /// The one line shown in the "while you were away" message.
    pub line: String,
    /// The incident's severity when it was held.
    pub severity: Severity,
    /// When it was held.
    pub held_at: Timestamp,
    /// Whether the incident resolved while held.
    pub resolved: bool,
}

/// Per-incident notification state, kept between events.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NotifyState {
    /// The card and thread per chat platform.
    #[serde(default)]
    pub messages: BTreeMap<String, ChatRefs>,
    /// When the card was first posted (or the alert first held).
    #[serde(default)]
    pub first_alert_at: Option<Timestamp>,
    /// When the last reminder went out.
    #[serde(default)]
    pub last_reminder_at: Option<Timestamp>,
    /// When the last "again" reply for a folded signal went out.
    #[serde(default)]
    pub last_fold_reply_at: Option<Timestamp>,
    /// Folded signals since that reply that were not mentioned yet.
    #[serde(default)]
    pub folds_since_reply: u32,
    /// The alert waiting for quiet hours to end, if any.
    #[serde(default)]
    pub held: Option<HeldItem>,
}

impl NotifyState {
    /// Whether an alert is waiting for quiet hours to end.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }
}
