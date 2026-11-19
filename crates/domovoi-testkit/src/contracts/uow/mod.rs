//! Contracts for the unit of work, run against the in-memory store and SQLite.

mod effects;
mod journal;
mod lifecycle;
mod lifecycle_retry;
mod lifecycle_scope;
mod rollback;
mod states;

pub use effects::{
    completing_an_unclaimed_effect_without_a_token_marks_it_done,
    duplicate_dedup_key_is_refused_while_pending, effect_completion_needs_the_current_claim_token,
    effect_id_must_be_unique, effects_are_listed_in_insertion_order, zero_max_attempts_is_refused,
};
pub use journal::{
    audit_draft_is_appended_with_the_next_sequence_number,
    discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals,
    incident_event_is_mirrored_into_the_journal_with_its_original_time,
    journal_sequence_follows_commit_order,
};
pub use lifecycle::{
    claim_prioritizes_pending_effects_before_expired_claims,
    claim_sets_token_generation_lease_and_attempt, claim_skips_effects_not_yet_due_and_other_kinds,
    claimed_effect_reclaimed_after_lease_expiry, dedup_key_blocks_a_second_pending_effect,
    dispatch_effects_are_never_reclaimed_after_lease_expiry,
    restart_reverts_claimed_effects_except_dispatch, stale_holder_cannot_reschedule_or_dead_letter,
    stale_lease_holder_cannot_complete_reclaimed_effect,
};
pub use lifecycle_retry::{
    attempt_limit_dead_letters_at_claim,
    expired_lease_beyond_attempt_limit_dies_instead_of_reclaim,
    reschedule_returns_effect_to_pending_with_backoff_time,
};
pub use lifecycle_scope::{
    an_empty_aggregate_scope_claims_nothing,
    cancel_effects_cancels_only_pending_effects_of_the_aggregate, claim_filters_by_aggregate_kind,
    revert_claimed_at_start_is_scoped_to_aggregates,
};
pub use rollback::failed_effect_completion_rolls_back_every_stage;
pub use states::{
    action_guard_refuses_a_changed_status, chat_token_hash_must_be_unique_across_actions,
    commit_applies_state_journal_audit_and_effects_together, empty_transaction_commits_nothing,
    later_failure_rolls_back_earlier_writes_journal_audit_and_effects,
    new_incident_over_existing_id_conflicts, repository_writes_advance_versions_like_commits,
    second_active_incident_for_a_fingerprint_is_refused_and_rolls_back,
    stale_incident_version_conflicts_and_changes_nothing,
};

use async_trait::async_trait;
use domovoi_core::EffectId;
use domovoi_core::ports::{
    ActionRepo, AuditLog, ClaimToken, EffectQueue, EffectRead, IncidentRepo, JournalRead, NoteRepo,
    SignalRepo, UnitOfWork, VersionedRead,
};
use jiff::Timestamp;

use super::ContractError;
use crate::InMemoryStore;
use crate::build::T0;

/// One store the unit-of-work contracts run against: SQLite or the in-memory fake.
#[async_trait]
pub trait UowSubject: Send + Sync {
    /// The unit of work.
    fn uow(&self) -> &dyn UnitOfWork;
    /// Versioned aggregate reads.
    fn versions(&self) -> &dyn VersionedRead;
    /// Journal reads.
    fn journal(&self) -> &dyn JournalRead;
    /// Effect reads.
    fn effects(&self) -> &dyn EffectRead;
    /// Incident writes.
    fn incidents(&self) -> &dyn IncidentRepo;
    /// Signal writes.
    fn signals(&self) -> &dyn SignalRepo;
    /// Action writes.
    fn actions(&self) -> &dyn ActionRepo;
    /// Note writes.
    fn notes(&self) -> &dyn NoteRepo;
    /// The audit log.
    fn audit(&self) -> &dyn AuditLog;
    /// Claiming and recovering effects: the queue the runner works from.
    fn queue(&self) -> &dyn EffectQueue;

    /// Test seam: puts a pending effect into `claimed` under `token`, as the effect runner will.
    async fn force_claim(
        &self,
        id: EffectId,
        token: ClaimToken,
        lease_until: Timestamp,
    ) -> Result<(), ContractError>;
}

#[async_trait]
impl UowSubject for InMemoryStore {
    fn uow(&self) -> &dyn UnitOfWork {
        self
    }

    fn versions(&self) -> &dyn VersionedRead {
        self
    }

    fn journal(&self) -> &dyn JournalRead {
        self
    }

    fn effects(&self) -> &dyn EffectRead {
        self
    }

    fn incidents(&self) -> &dyn IncidentRepo {
        self
    }

    fn signals(&self) -> &dyn SignalRepo {
        self
    }

    fn actions(&self) -> &dyn ActionRepo {
        self
    }

    fn notes(&self) -> &dyn NoteRepo {
        self
    }

    fn audit(&self) -> &dyn AuditLog {
        self
    }

    fn queue(&self) -> &dyn EffectQueue {
        self
    }

    async fn force_claim(
        &self,
        id: EffectId,
        token: ClaimToken,
        lease_until: Timestamp,
    ) -> Result<(), ContractError> {
        Ok(self.claim_effect(id, token, lease_until)?)
    }
}

/// Keeps the timestamp import used by every contract module in one place.
pub(super) fn lease(secs: i64) -> Timestamp {
    T0.checked_add(jiff::SignedDuration::from_secs(secs))
        .unwrap_or(T0)
}
