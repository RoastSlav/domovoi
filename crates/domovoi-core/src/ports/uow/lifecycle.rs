//! Claiming and recovering effects: the parts of the work outbox that do not travel inside an
//! owner's transaction.
//!
//! Completion, reschedule and dead-letter are fenced updates inside the owner's
//! [`Transaction`](super::Transaction), so the state change and the effect's progress cannot
//! disagree. This module holds the two operations a runner does on its own: claiming due work
//! under a fresh token, and putting back what a previous session left claimed.

use async_trait::async_trait;
use jiff::{SignedDuration, Timestamp};

use crate::id::EffectId;
use crate::ports::repos::StoreError;

use super::{ClaimToken, EffectKind, EffectRecord};

/// An effect this runner now holds, with the token that fences it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedEffect {
    /// The stored row, already `claimed`, with its attempt and generation updated.
    pub record: EffectRecord,
    /// The claim token every later write must name.
    pub token: ClaimToken,
}

/// What one claim pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaimBatch {
    /// Effects this runner now holds.
    pub claimed: Vec<ClaimedEffect>,
    /// Effects found past their attempt limit and moved to `dead` by this pass.
    pub dead: Vec<EffectRecord>,
}

/// Reschedules a claimed effect for a later attempt, inside its owner's transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectReschedule {
    /// The effect to put back.
    pub id: EffectId,
    /// The claim the caller holds; a stale token changes nothing.
    pub claim_token: ClaimToken,
    /// When the effect may be claimed again.
    pub not_before: Timestamp,
    /// Why the attempt failed. Stored in `last_error`.
    pub error: String,
    /// When the reschedule happened.
    pub at: Timestamp,
}

/// Dead-letters a claimed effect, inside its owner's transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDeath {
    /// The effect to give up on.
    pub id: EffectId,
    /// The claim the caller holds; a stale token changes nothing.
    pub claim_token: ClaimToken,
    /// Why the effect was given up on. Stored in `last_error`.
    pub error: String,
    /// When it died.
    pub at: Timestamp,
}

/// Claims and recovers effects.
///
/// Completion, reschedule and dead-letter are not here: they travel inside the owner's
/// [`Transaction`](super::Transaction) so they commit with the state change they belong to.
#[async_trait]
pub trait EffectQueue: Send + Sync {
    /// Claims up to `limit` effects whose kind is in `kinds`: first `pending` ones that are
    /// due, then `claimed` ones whose lease expired (never a `dispatch` effect). Each claimed
    /// row gets a fresh token, the next generation and a lease; each row already past its
    /// attempt limit is dead-lettered instead.
    async fn claim_due(
        &self,
        runner: &str,
        kinds: &[EffectKind],
        now: Timestamp,
        lease: SignedDuration,
        limit: u16,
    ) -> Result<ClaimBatch, StoreError>;

    /// At start, returns every `claimed` effect whose kind is not `dispatch` to `pending` with
    /// a new generation and no token. Returns how many were reverted.
    async fn revert_claimed_at_start(&self) -> Result<u64, StoreError>;
}
