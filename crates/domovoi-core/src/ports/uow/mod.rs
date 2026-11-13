//! The unit of work: one atomic commit for state, journal, audit and effects.

mod effects;
mod journal;
mod transaction;

pub use effects::{
    ClaimToken, EffectCompletion, EffectDraft, EffectKind, EffectRead, EffectRecord, EffectStatus,
};
pub use journal::{
    AuditDraft, JournalDraft, JournalEntry, JournalKind, JournalRead, timeline_journal,
};
pub use transaction::{
    ActionGuard, AggregateKind, AggregateRef, CommitError, CommitReceipt, Expectation, Expected,
    Observed, StateWrite, Transaction, Versioned,
};

use async_trait::async_trait;

use crate::id::{IncidentId, NoteId};
use crate::ports::repos::StoreError;
use crate::{Incident, Note};

/// Commits a [`Transaction`] atomically.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Applies the whole transaction or none of it. An empty transaction succeeds without
    /// touching the store and returns an empty receipt.
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError>;
}

/// Reads aggregates together with the version a later write must name.
#[async_trait]
pub trait VersionedRead: Send + Sync {
    /// The incident and its version. Body and version come from one statement or one snapshot.
    async fn incident(&self, id: IncidentId) -> Result<Option<Versioned<Incident>>, StoreError>;

    /// Every active (not resolved) incident with its version, newest `opened_at` first.
    async fn active_incidents(&self) -> Result<Vec<Versioned<Incident>>, StoreError>;

    /// The note and its version.
    async fn note(&self, id: NoteId) -> Result<Option<Versioned<Note>>, StoreError>;
}
