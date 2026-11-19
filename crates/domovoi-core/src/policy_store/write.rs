//! The single `StateWrite` payload for policy.

use jiff::Timestamp;

use super::shadow::{ShadowRecord, SweepStats};
use super::version::{AckRecord, StoredRule};
use crate::actor::Actor;
use crate::id::PolicyVersionId;

/// What a draft save expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftGuard {
    /// No draft may exist.
    Absent,
    /// The stored draft is at this revision.
    Revision(u64),
}

/// One change to the policy store.
///
/// The first three variants are built only by the hub's policy service; `RecordShadow` only
/// by the shadow recorder in the engine. The store applies whatever it is given.
#[derive(Debug, Clone)]
pub enum PolicyWrite {
    /// Creates the draft or replaces all of its rules.
    SaveDraft {
        /// The draft's id (used when creating).
        id: PolicyVersionId,
        /// What the stored draft must look like.
        expected: DraftGuard,
        /// The active version number when the draft is created.
        based_on: Option<u32>,
        /// The complete rule set.
        rules: Vec<StoredRule>,
        /// The hash of `rules`.
        rules_hash: String,
        /// Who saved.
        actor: Actor,
        /// When.
        at: Timestamp,
    },
    /// Deletes the draft.
    DiscardDraft {
        /// The draft's id.
        id: PolicyVersionId,
        /// The revision the caller read.
        expected_revision: u64,
    },
    /// Turns the draft into the next numbered version.
    Publish {
        /// The draft's id.
        id: PolicyVersionId,
        /// The revision the caller read.
        expected_revision: u64,
        /// The number to assign; must be one more than the highest published.
        number: u32,
        /// The acknowledged findings, stored with the version.
        acknowledgements: Vec<AckRecord>,
        /// Who published.
        actor: Actor,
        /// When.
        at: Timestamp,
    },
    /// Stores shadow decisions; a row for an execution that already has one is kept as it is.
    /// `error` is set (with no records) when a sweep failed, so the failure stays visible.
    RecordShadow {
        /// The decisions of one batch.
        records: Vec<ShadowRecord>,
        /// Counts of the batch.
        sweep: SweepStats,
        /// When the batch was committed.
        at: Timestamp,
        /// Why the sweep failed, if it did.
        error: Option<String>,
    },
}
