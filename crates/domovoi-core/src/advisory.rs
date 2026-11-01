//! Advisories: low-priority hygiene findings that appear in the digest and never notify.

use jiff::Timestamp;

use crate::id::{AdvisoryId, HostId};

/// The kind of hygiene finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryKind {
    /// An agent stopped sending heartbeats.
    HeartbeatMissing,
    /// Triage keeps failing and falls back to raw cards.
    TriageDegraded,
    /// A container runs an old image.
    StaleImage,
    /// A compose stack has no source copy.
    StackWithoutSource,
    /// A secret was marked as exposed.
    SecretExposed,
    /// A baseline entry expired because it was not seen.
    BaselineEntryExpired,
    /// An agent runs an older version than the hub.
    AgentOutdated,
    /// A chat gateway or notifier could not accept a delivery after every retry.
    DeliveryFailed,
}

/// One hygiene finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Advisory {
    /// Unique id.
    pub id: AdvisoryId,
    /// What kind of finding.
    pub kind: AdvisoryKind,
    /// The host it is about, if any.
    pub host: Option<HostId>,
    /// One-line title.
    pub title: String,
    /// What to do about it.
    pub detail: String,
    /// When it was raised.
    pub created_at: Timestamp,
    /// When it cleared.
    pub resolved_at: Option<Timestamp>,
}
