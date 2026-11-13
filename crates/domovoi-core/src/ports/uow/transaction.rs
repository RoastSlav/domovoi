//! One atomic business event: typed state writes, journal entries, audit entries and effects.

use std::fmt;

use crate::id::{ActionId, EffectId, IncidentId, NoteId};
use crate::ports::repos::IncidentTimelineEntry;
use crate::{ActionRequest, Incident, Note, Signal};

use super::{AuditDraft, EffectCompletion, EffectDraft, JournalDraft};

/// Which kind of aggregate a write, journal entry or effect belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregateKind {
    /// An incident.
    Incident,
    /// An action request.
    Action,
    /// A runbook note.
    Note,
}

impl AggregateKind {
    /// Stable lowercase name stored in the database: `"incident"`, `"action"` or `"note"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::Action => "action",
            Self::Note => "note",
        }
    }

    /// Reads [`AggregateKind::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "incident" => Some(Self::Incident),
            "action" => Some(Self::Action),
            "note" => Some(Self::Note),
            _ => None,
        }
    }
}

/// An aggregate instance: kind plus id text (a UUID for every kind here).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AggregateRef {
    /// What kind of aggregate this is.
    pub kind: AggregateKind,
    /// The aggregate's id, as text.
    pub id: String,
}

impl AggregateRef {
    /// An incident reference.
    #[must_use]
    pub fn incident(id: IncidentId) -> Self {
        Self {
            kind: AggregateKind::Incident,
            id: id.to_string(),
        }
    }

    /// An action reference.
    #[must_use]
    pub fn action(id: ActionId) -> Self {
        Self {
            kind: AggregateKind::Action,
            id: id.to_string(),
        }
    }

    /// A note reference.
    #[must_use]
    pub fn note(id: NoteId) -> Self {
        Self {
            kind: AggregateKind::Note,
            id: id.to_string(),
        }
    }
}

impl fmt::Display for AggregateRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.id)
    }
}

/// What a versioned write expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// No row with this id exists yet.
    New,
    /// The stored row is at this version.
    Version(u64),
}

impl From<Expected> for Expectation {
    fn from(value: Expected) -> Self {
        match value {
            Expected::New => Self::New,
            Expected::Version(version) => Self::Version(version),
        }
    }
}

/// What an action write expects to find; actions are guarded by their status name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionGuard {
    /// No action with this id exists yet.
    New,
    /// The stored action has this status name, for example `"proposed"`.
    Status(String),
}

impl From<ActionGuard> for Expectation {
    fn from(value: ActionGuard) -> Self {
        match value {
            ActionGuard::New => Self::New,
            ActionGuard::Status(status) => Self::Status(status),
        }
    }
}

/// The expectation recorded in a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation {
    /// The write expected a new row.
    New,
    /// The write expected this version.
    Version(u64),
    /// The write expected this action status name.
    Status(String),
}

/// What was actually stored when a conflict happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    /// Nothing is stored under the id.
    Missing,
    /// The stored row is at this version.
    Version(u64),
    /// The stored action has this status name.
    Status(String),
}

/// A value and the stored version it was read at.
#[derive(Debug, Clone, PartialEq)]
pub struct Versioned<T> {
    /// The stored value.
    pub value: T,
    /// The version a later write must name.
    pub version: u64,
}

/// One typed state change.
///
/// Each variant is meant to be built only by the writer that owns the aggregate: the incident
/// manager for incidents and their signals, the action writer for actions, and the note writer
/// for notes. The unit of work applies whatever it is given; it does not decide ownership.
#[derive(Debug, Clone)]
pub enum StateWrite {
    /// Stores a signal and, if given, links it to an incident.
    Signal {
        /// The signal to store.
        signal: Signal,
        /// The incident to link it to, if any.
        incident: Option<IncidentId>,
    },
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`) an incident.
    Incident {
        /// The incident to store.
        incident: Incident,
        /// What the stored row must look like.
        expected: Expected,
    },
    /// Appends a timeline entry. Also writes a mirrored journal row in the same transaction.
    IncidentEvent(IncidentTimelineEntry),
    /// Deletes a pending incident with its timeline and signal links.
    DiscardPendingIncident {
        /// The incident to discard.
        id: IncidentId,
        /// The version the caller read.
        expected: u64,
    },
    /// Inserts (`ActionGuard::New`) or replaces an action whose stored status is as guarded.
    Action {
        /// The action to store.
        action: ActionRequest,
        /// What the stored action must look like.
        expected: ActionGuard,
    },
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`) a note.
    Note {
        /// The note to store.
        note: Note,
        /// What the stored row must look like.
        expected: Expected,
    },
}

/// Everything one business event changes.
///
/// Applied in this order: `writes`, then the mirrored and explicit journal rows, then `audit`,
/// then `new_effects`, then `completed_effects`.
#[derive(Debug, Clone, Default)]
pub struct Transaction {
    /// State changes, applied in order.
    pub writes: Vec<StateWrite>,
    /// Journal rows to append, after the rows mirrored from timeline writes.
    pub journal: Vec<JournalDraft>,
    /// Audit entries to append.
    pub audit: Vec<AuditDraft>,
    /// Effects to create.
    pub new_effects: Vec<EffectDraft>,
    /// Effects to complete.
    pub completed_effects: Vec<EffectCompletion>,
}

impl Transaction {
    /// An empty transaction.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this transaction changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
            && self.journal.is_empty()
            && self.audit.is_empty()
            && self.new_effects.is_empty()
            && self.completed_effects.is_empty()
    }

    /// Adds one state write.
    #[must_use]
    pub fn write(mut self, write: StateWrite) -> Self {
        self.writes.push(write);
        self
    }

    /// Adds one journal entry.
    #[must_use]
    pub fn journal(mut self, draft: JournalDraft) -> Self {
        self.journal.push(draft);
        self
    }

    /// Adds one audit entry.
    #[must_use]
    pub fn audit(mut self, draft: AuditDraft) -> Self {
        self.audit.push(draft);
        self
    }

    /// Adds one new effect.
    #[must_use]
    pub fn effect(mut self, draft: EffectDraft) -> Self {
        self.new_effects.push(draft);
        self
    }

    /// Adds one effect completion.
    #[must_use]
    pub fn complete(mut self, completion: EffectCompletion) -> Self {
        self.completed_effects.push(completion);
        self
    }
}

/// What a successful commit assigned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReceipt {
    /// Journal sequence numbers: mirrored timeline rows in write order, then explicit drafts in
    /// draft order.
    pub journal_seqs: Vec<u64>,
    /// Audit sequence numbers in draft order.
    pub audit_seqs: Vec<u64>,
    /// New version of every versioned aggregate written (incidents, notes), in write order.
    pub versions: Vec<(AggregateRef, u64)>,
}

/// Why a commit changed nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommitError {
    /// A guarded write found something other than what the caller read.
    #[error("{aggregate} changed meanwhile: expected {expected:?}, found {found:?}")]
    Conflict {
        /// The aggregate that was written.
        aggregate: AggregateRef,
        /// What the caller expected.
        expected: Expectation,
        /// What the store holds.
        found: Observed,
    },
    /// A uniqueness, foreign-key or state rule refused a write; the text names it.
    #[error("constraint violated: {0}")]
    Constraint(String),
    /// A completion named an effect that is not claimed under that token (or not pending, for
    /// an unclaimed completion): the caller is a stale lease holder.
    #[error("effect {effect} is not held by this caller")]
    StaleEffect {
        /// The effect that could not be completed.
        effect: EffectId,
    },
    /// The store failed. Nothing was written.
    #[error("store error: {0}")]
    Backend(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_kinds_round_trip() {
        for kind in [
            AggregateKind::Incident,
            AggregateKind::Action,
            AggregateKind::Note,
        ] {
            assert_eq!(AggregateKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(AggregateKind::parse("host"), None);
    }

    #[test]
    fn aggregate_ref_displays_as_kind_and_id() {
        let id = IncidentId::from_uuid(uuid::Uuid::from_u128(7));
        assert_eq!(
            AggregateRef::incident(id).to_string(),
            format!("incident:{id}")
        );
    }

    #[test]
    fn an_empty_transaction_reports_itself_empty() {
        assert!(Transaction::new().is_empty());
        assert!(
            !Transaction::default()
                .write(StateWrite::DiscardPendingIncident {
                    id: IncidentId::new(),
                    expected: 1,
                })
                .is_empty()
        );
    }
}
