//! The work outbox: pending side effects created and completed inside a transaction.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

use crate::id::EffectId;
use crate::ports::repos::StoreError;

use super::AggregateRef;

/// The kinds of pending work.
///
/// Stored as snake_case strings and validated in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// Gather the context a model needs.
    BuildContext,
    /// Ask a model.
    CallModel,
    /// Collect more evidence from an agent.
    CollectEvidence,
    /// Evaluate a policy.
    EvaluatePolicy,
    /// Issue a permit to an agent.
    IssuePermit,
    /// Send a command to an agent.
    Dispatch,
    /// Reconcile what an agent reported.
    Reconcile,
    /// Verify that an action took effect.
    Verify,
    /// Send a notification.
    Notify,
    /// Wake something that is waiting.
    Wake,
    /// Draft a summary.
    DraftSummary,
}

impl EffectKind {
    /// Stable snake_case name stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BuildContext => "build_context",
            Self::CallModel => "call_model",
            Self::CollectEvidence => "collect_evidence",
            Self::EvaluatePolicy => "evaluate_policy",
            Self::IssuePermit => "issue_permit",
            Self::Dispatch => "dispatch",
            Self::Reconcile => "reconcile",
            Self::Verify => "verify",
            Self::Notify => "notify",
            Self::Wake => "wake",
            Self::DraftSummary => "draft_summary",
        }
    }

    /// Reads [`EffectKind::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "build_context" => Some(Self::BuildContext),
            "call_model" => Some(Self::CallModel),
            "collect_evidence" => Some(Self::CollectEvidence),
            "evaluate_policy" => Some(Self::EvaluatePolicy),
            "issue_permit" => Some(Self::IssuePermit),
            "dispatch" => Some(Self::Dispatch),
            "reconcile" => Some(Self::Reconcile),
            "verify" => Some(Self::Verify),
            "notify" => Some(Self::Notify),
            "wake" => Some(Self::Wake),
            "draft_summary" => Some(Self::DraftSummary),
            _ => None,
        }
    }
}

/// Effect lifecycle states.
///
/// Only [`EffectStatus::Pending`] and [`EffectStatus::Done`] are produced by transactions in this
/// release; a later release owns claiming, retries and dead-lettering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectStatus {
    /// Waiting to be claimed.
    Pending,
    /// Claimed by a runner under a [`ClaimToken`].
    Claimed,
    /// Finished.
    Done,
    /// Gave up after too many attempts.
    Dead,
    /// No longer needed.
    Cancelled,
}

impl EffectStatus {
    /// Stable lowercase name stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Done => "done",
            Self::Dead => "dead",
            Self::Cancelled => "cancelled",
        }
    }

    /// Reads [`EffectStatus::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pending" => Some(Self::Pending),
            "claimed" => Some(Self::Claimed),
            "done" => Some(Self::Done),
            "dead" => Some(Self::Dead),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Identifies one claim of an effect. A fresh value per claim; fences stale lease holders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ClaimToken(uuid::Uuid);

impl ClaimToken {
    /// A fresh random claim token.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Wraps an existing UUID, for example one read from storage.
    #[must_use]
    pub const fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the underlying UUID.
    #[must_use]
    pub const fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for ClaimToken {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ClaimToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ClaimToken {
    type Err = uuid::Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::from_str(text).map(Self)
    }
}

/// A new pending effect, created in its owner's transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDraft {
    /// The effect's id.
    pub id: EffectId,
    /// What kind of work it is.
    pub kind: EffectKind,
    /// What it is about.
    pub aggregate: AggregateRef,
    /// Structured input for the worker.
    pub payload: Value,
    /// When it may first be claimed.
    pub not_before: Timestamp,
    /// Attempts allowed before dead-lettering. Must be at least 1.
    pub max_attempts: u32,
    /// While an effect with this key is pending or claimed, a second one is refused.
    pub dedup_key: Option<String>,
    /// When it was created.
    pub created_at: Timestamp,
}

/// Completes an effect inside the owner's transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectCompletion {
    /// The effect to complete.
    pub id: EffectId,
    /// `Some(token)`: the effect must be claimed under exactly this token. `None`: the effect
    /// must still be pending.
    pub claim_token: Option<ClaimToken>,
    /// When it completed.
    pub at: Timestamp,
}

/// A stored effect row. Fields mirror the columns of the `effects` table.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRecord {
    /// The effect's id.
    pub id: EffectId,
    /// The stable kind string as stored.
    pub kind: String,
    /// What it is about.
    pub aggregate: AggregateRef,
    /// Structured input for the worker.
    pub payload: Value,
    /// The stable status string as stored.
    pub status: String,
    /// When it may first be claimed.
    pub not_before: Timestamp,
    /// Attempts made so far.
    pub attempt: u32,
    /// Attempts allowed before dead-lettering.
    pub max_attempts: u32,
    /// The de-duplication key, if any.
    pub dedup_key: Option<String>,
    /// The current claim token, if claimed.
    pub claim_token: Option<ClaimToken>,
    /// How many times the effect has been claimed.
    pub claim_generation: u64,
    /// When the current claim expires.
    pub lease_until: Option<Timestamp>,
    /// The runner holding the current claim.
    pub runner_id: Option<String>,
    /// The last error a worker reported.
    pub last_error: Option<String>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it completed.
    pub completed_at: Option<Timestamp>,
}

/// Reads the effect outbox.
#[async_trait]
pub trait EffectRead: Send + Sync {
    /// The effect with `id`.
    async fn effect(&self, id: EffectId) -> Result<Option<EffectRecord>, StoreError>;

    /// Up to `limit` effects in `status`, in creation order (oldest first). `limit` is clamped
    /// to 1..=500.
    async fn effects_with_status(
        &self,
        status: EffectStatus,
        limit: u16,
    ) -> Result<Vec<EffectRecord>, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_kinds_round_trip() {
        let kinds = [
            EffectKind::BuildContext,
            EffectKind::CallModel,
            EffectKind::CollectEvidence,
            EffectKind::EvaluatePolicy,
            EffectKind::IssuePermit,
            EffectKind::Dispatch,
            EffectKind::Reconcile,
            EffectKind::Verify,
            EffectKind::Notify,
            EffectKind::Wake,
            EffectKind::DraftSummary,
        ];
        for kind in kinds {
            assert_eq!(EffectKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EffectKind::parse("explode"), None);
    }

    #[test]
    fn effect_statuses_round_trip() {
        let statuses = [
            EffectStatus::Pending,
            EffectStatus::Claimed,
            EffectStatus::Done,
            EffectStatus::Dead,
            EffectStatus::Cancelled,
        ];
        for status in statuses {
            assert_eq!(EffectStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(EffectStatus::parse("running"), None);
    }

    #[test]
    fn claim_tokens_round_trip_through_display_and_from_str() {
        let token = ClaimToken::new();
        let parsed: ClaimToken = token.to_string().parse().unwrap();
        assert_eq!(token, parsed);
    }
}
