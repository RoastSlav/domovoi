//! Contracts for the unit of work, run against the in-memory store and SQLite.

mod effects;
mod journal;
mod states;

pub use effects::{
    completing_an_unclaimed_effect_without_a_token_marks_it_done,
    duplicate_dedup_key_is_refused_while_pending, effect_completion_needs_the_current_claim_token,
    zero_max_attempts_is_refused,
};
pub use journal::{
    audit_draft_is_appended_with_the_next_sequence_number,
    discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals,
    incident_event_is_mirrored_into_the_journal_with_its_original_time,
    journal_sequence_follows_commit_order,
};
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
    ActionRepo, AuditLog, ClaimToken, EffectRead, IncidentRepo, JournalRead, NoteRepo, SignalRepo,
    UnitOfWork, VersionedRead,
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
