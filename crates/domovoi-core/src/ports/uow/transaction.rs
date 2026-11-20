//! One atomic business event: typed state writes, journal entries, audit entries and effects.

use std::fmt;

use crate::id::{ActionId, EffectId, IncidentId, NoteId, OperationId, PolicyVersionId};
use crate::policy_store::PolicyWrite;
use crate::ports::repos::IncidentTimelineEntry;
use crate::{ActionRequest, Incident, Note, Signal};

use super::{
    AuditDraft, EffectCancel, EffectCompletion, EffectDeath, EffectDraft, EffectReschedule,
    JournalDraft, OperationWrite,
};

/// Which kind of aggregate a write, journal entry or effect belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregateKind {
    /// An incident.
    Incident,
    /// An action request.
    Action,
    /// A runbook note.
    Note,
    /// An operation.
    Operation,
    /// A policy version, draft or published.
    Policy,
    /// A knowledge fact.
    Fact,
    /// A knowledge relation.
    Relation,
    /// A knowledge entity.
    Entity,
}

impl AggregateKind {
    /// The aggregates the incident and action services own. The legacy effect runner claims
    /// only these, so it never touches an operation's effects.
    pub const LEGACY: [Self; 3] = [Self::Incident, Self::Action, Self::Note];

    /// Stable lowercase name stored in the database: `"incident"`, `"action"`, `"note"` or
    /// `"operation"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::Action => "action",
            Self::Note => "note",
            Self::Operation => "operation",
            Self::Policy => "policy",
            Self::Fact => "fact",
            Self::Relation => "relation",
            Self::Entity => "entity",
        }
    }

    /// Reads [`AggregateKind::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "incident" => Some(Self::Incident),
            "action" => Some(Self::Action),
            "note" => Some(Self::Note),
            "operation" => Some(Self::Operation),
            "policy" => Some(Self::Policy),
            "fact" => Some(Self::Fact),
            "relation" => Some(Self::Relation),
            "entity" => Some(Self::Entity),
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

impl AggregateRef {
    /// An operation reference: the aggregate of every operation journal entry and effect.
    #[must_use]
    pub fn operation(id: OperationId) -> Self {
        Self {
            kind: AggregateKind::Operation,
            id: id.to_string(),
        }
    }
}

impl AggregateRef {
    /// A knowledge fact reference.
    #[must_use]
    pub fn fact(id: crate::id::FactRecordId) -> Self {
        Self {
            kind: AggregateKind::Fact,
            id: id.to_string(),
        }
    }

    /// A knowledge relation reference.
    #[must_use]
    pub fn relation(id: crate::id::RelationId) -> Self {
        Self {
            kind: AggregateKind::Relation,
            id: id.to_string(),
        }
    }

    /// A knowledge entity reference.
    #[must_use]
    pub fn entity(id: crate::id::EntityId) -> Self {
        Self {
            kind: AggregateKind::Entity,
            id: id.to_string(),
        }
    }
}

impl AggregateRef {
    /// A policy version reference: the aggregate of a `policy.published` journal entry.
    #[must_use]
    pub fn policy(id: PolicyVersionId) -> Self {
        Self {
            kind: AggregateKind::Policy,
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
/// manager for incidents and their signals, the action writer for actions, the note writer
/// for notes, and the operation writer for operations. The unit of work applies whatever it is given; it does not decide ownership.
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
    /// One change to an operation, its steps, edges, executions or incident links.
    Operation(OperationWrite),
    /// One change to the policy store: a draft, a publication or a batch of shadow decisions.
    Policy(PolicyWrite),
    /// One change to the knowledge store: a fact, a relation, an entity or a confirmation.
    Knowledge(crate::knowledge::KnowledgeWrite),
}

/// Everything one business event changes.
///
/// Applied in this order: `writes`, then the mirrored and explicit journal rows, then `audit`,
/// then `cancelled_effects`, `new_effects`, `completed_effects`, `rescheduled_effects` and
/// `dead_effects`. Cancelling before creating means an aggregate's cancellation never removes
/// an effect the same transaction creates.
#[derive(Debug, Clone, Default)]
pub struct Transaction {
    /// State changes, applied in order.
    pub writes: Vec<StateWrite>,
    /// Journal rows to append, after the rows mirrored from timeline writes.
    pub journal: Vec<JournalDraft>,
    /// Audit entries to append.
    pub audit: Vec<AuditDraft>,
    /// Aggregates whose pending effects are cancelled.
    pub cancelled_effects: Vec<EffectCancel>,
    /// Effects to create.
    pub new_effects: Vec<EffectDraft>,
    /// Effects to complete.
    pub completed_effects: Vec<EffectCompletion>,
    /// Claimed effects to put back for a later attempt.
    pub rescheduled_effects: Vec<EffectReschedule>,
    /// Claimed effects to dead-letter.
    pub dead_effects: Vec<EffectDeath>,
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
            && self.cancelled_effects.is_empty()
            && self.new_effects.is_empty()
            && self.completed_effects.is_empty()
            && self.rescheduled_effects.is_empty()
            && self.dead_effects.is_empty()
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

    /// Cancels every pending effect of one aggregate.
    #[must_use]
    pub fn cancel_effects(mut self, cancel: EffectCancel) -> Self {
        self.cancelled_effects.push(cancel);
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

    /// Adds one fenced reschedule of a claimed effect.
    #[must_use]
    pub fn reschedule(mut self, reschedule: EffectReschedule) -> Self {
        self.rescheduled_effects.push(reschedule);
        self
    }

    /// Adds one fenced dead-lettering of a claimed effect.
    #[must_use]
    pub fn kill(mut self, death: EffectDeath) -> Self {
        self.dead_effects.push(death);
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
    use jiff::Timestamp;

    use super::super::ClaimToken;
    use super::*;

    #[test]
    fn aggregate_kinds_round_trip() {
        for kind in [
            AggregateKind::Incident,
            AggregateKind::Action,
            AggregateKind::Note,
            AggregateKind::Operation,
            AggregateKind::Policy,
        ] {
            assert_eq!(AggregateKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(AggregateKind::parse("host"), None);
        assert_eq!(
            AggregateKind::LEGACY,
            [
                AggregateKind::Incident,
                AggregateKind::Action,
                AggregateKind::Note
            ]
        );
    }

    #[test]
    fn a_cancellation_alone_makes_a_transaction_non_empty() {
        let cancel = EffectCancel {
            aggregate: AggregateRef::operation(OperationId::new()),
            at: Timestamp::constant(1_700_000_000, 0),
        };
        assert!(!Transaction::new().cancel_effects(cancel).is_empty());
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

    #[test]
    fn a_transaction_with_only_effect_lifecycle_rows_is_not_empty() {
        let token = ClaimToken::new();
        assert!(
            !Transaction::new()
                .reschedule(EffectReschedule {
                    id: EffectId::new(),
                    claim_token: token,
                    not_before: Timestamp::constant(1_700_000_000, 0),
                    error: "offline".to_owned(),
                    at: Timestamp::constant(1_700_000_000, 0),
                })
                .is_empty()
        );
        assert!(
            !Transaction::new()
                .kill(EffectDeath {
                    id: EffectId::new(),
                    claim_token: token,
                    error: "gave up".to_owned(),
                    at: Timestamp::constant(1_700_000_000, 0),
                })
                .is_empty()
        );
    }
}
