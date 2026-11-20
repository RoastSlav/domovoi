//! The operational journal: what happened, in commit order, and the audit entries that share the
//! same transaction.

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

use crate::actor::{Actor, SYSTEM_INCIDENTS};
use crate::ports::repos::{AuditEvent, IncidentTimelineEntry, StoreError};

use super::AggregateRef;

/// Typed journal kinds. Closed on purpose; later releases add variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalKind {
    /// A line in an incident's timeline.
    IncidentTimeline,
    /// An action changed status.
    ActionTransition,
    /// An operation was created.
    OperationCreated,
    /// An operation changed status.
    OperationStatus,
    /// A step was added to an operation.
    StepCreated,
    /// A step started.
    StepStarted,
    /// A step finished.
    StepFinished,
    /// An execution moved to another state.
    ExecutionTransition,
    /// A signal folded into an existing incident.
    IncidentFolded,
    /// A policy version was published.
    PolicyPublished,
    /// A knowledge fact changed.
    FactChange,
    /// A knowledge relation changed.
    RelationChange,
    /// A knowledge entity changed.
    EntityChange,
}

impl JournalKind {
    /// Stable dotted name stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IncidentTimeline => "incident.timeline",
            Self::ActionTransition => "action.transition",
            Self::OperationCreated => "operation.created",
            Self::OperationStatus => "operation.status",
            Self::StepCreated => "operation.step_created",
            Self::StepStarted => "operation.step_started",
            Self::StepFinished => "operation.step_finished",
            Self::ExecutionTransition => "operation.execution",
            Self::IncidentFolded => "incident.folded",
            Self::PolicyPublished => "policy.published",
            Self::FactChange => "fact.changed",
            Self::RelationChange => "relation.changed",
            Self::EntityChange => "entity.changed",
        }
    }

    /// Reads [`JournalKind::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "incident.timeline" => Some(Self::IncidentTimeline),
            "action.transition" => Some(Self::ActionTransition),
            "operation.created" => Some(Self::OperationCreated),
            "operation.status" => Some(Self::OperationStatus),
            "operation.step_created" => Some(Self::StepCreated),
            "operation.step_started" => Some(Self::StepStarted),
            "operation.step_finished" => Some(Self::StepFinished),
            "operation.execution" => Some(Self::ExecutionTransition),
            "incident.folded" => Some(Self::IncidentFolded),
            "policy.published" => Some(Self::PolicyPublished),
            "fact.changed" => Some(Self::FactChange),
            "relation.changed" => Some(Self::RelationChange),
            "entity.changed" => Some(Self::EntityChange),
            _ => None,
        }
    }
}

/// A journal row to append. The store assigns `seq`.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalDraft {
    /// What the entry is about.
    pub aggregate: AggregateRef,
    /// Which kind of entry this is.
    pub kind: JournalKind,
    /// Schema version of `payload` for this kind. Starts at 1.
    pub payload_v: u32,
    /// Structured detail.
    pub payload: Value,
    /// Who did it.
    pub actor: Actor,
    /// When it happened.
    pub occurred_at: Timestamp,
}

/// A stored journal row.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    /// The global sequence number.
    pub seq: u64,
    /// What the entry is about.
    pub aggregate: AggregateRef,
    /// The stable kind string as stored. Readers must tolerate kinds they do not know.
    pub kind: String,
    /// Schema version of `payload`.
    pub payload_v: u32,
    /// Structured detail.
    pub payload: Value,
    /// Who did it.
    pub actor: String,
    /// When it happened.
    pub occurred_at: Timestamp,
}

/// An audit entry to append in the same transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditDraft {
    /// Who did it.
    pub actor: Actor,
    /// When it happened.
    pub at: Timestamp,
    /// Dotted kind, for example `action.approved`.
    pub kind: String,
    /// What it was done to, for example `action:<id>`.
    pub subject: String,
    /// Structured detail; `Value::Null` when there is none.
    pub detail: Value,
}

impl AuditDraft {
    /// Builds an audit entry for a transaction.
    #[must_use]
    pub fn new(actor: Actor, at: Timestamp, kind: String, subject: String, detail: Value) -> Self {
        Self {
            actor,
            at,
            kind,
            subject,
            detail,
        }
    }

    /// The event to store, with the actor in its string form.
    #[must_use]
    pub fn into_event(self) -> AuditEvent {
        AuditEvent {
            at: self.at,
            actor: self.actor.to_string(),
            kind: self.kind,
            subject: self.subject,
            detail: self.detail,
        }
    }
}

/// The journal row that mirrors a timeline entry.
///
/// The aggregate is the incident, the kind is [`JournalKind::IncidentTimeline`], `payload_v` is 1,
/// the actor is `"system:incidents"` and the time is the entry's. The payload is exactly
/// `{"kind": entry.kind, "summary": entry.summary, "detail": entry.detail}`.
#[must_use]
pub fn timeline_journal(entry: &IncidentTimelineEntry) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::incident(entry.incident),
        kind: JournalKind::IncidentTimeline,
        payload_v: 1,
        payload: serde_json::json!({
            "kind": entry.kind,
            "summary": entry.summary,
            "detail": entry.detail,
        }),
        actor: Actor::System(SYSTEM_INCIDENTS.to_owned()),
        occurred_at: entry.at,
    }
}

/// The journal as a change stream: what is gone, and a way to wait for what is new.
#[async_trait]
pub trait JournalFeed: JournalRead {
    /// The highest `seq` that retention has ever deleted; 0 when nothing was pruned. A reader
    /// whose cursor is below this may have missed deleted rows and must refetch.
    async fn floor_seq(&self) -> Result<u64, StoreError>;

    /// Completes once the committed head is greater than `seq`; at once if it already is.
    /// Cancel-safe. Never reads rows: the caller re-reads with `entries_after`.
    async fn wait_after(&self, seq: u64);
}

/// Reads the journal.
#[async_trait]
pub trait JournalRead: Send + Sync {
    /// Up to `limit` entries with `seq > after_seq`, oldest first. `limit` is clamped to
    /// 1..=500.
    async fn entries_after(
        &self,
        after_seq: u64,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError>;

    /// Up to `limit` entries of one aggregate, oldest first. `limit` is clamped to 1..=500.
    async fn entries_for(
        &self,
        aggregate: &AggregateRef,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError>;

    /// The highest assigned `seq` (never lower than the pruned floor), 0 when the journal has
    /// never held a row.
    async fn head_seq(&self) -> Result<u64, StoreError>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::id::IncidentId;

    use super::*;

    #[test]
    fn journal_kinds_round_trip() {
        for kind in [
            JournalKind::IncidentTimeline,
            JournalKind::ActionTransition,
            JournalKind::OperationCreated,
            JournalKind::OperationStatus,
            JournalKind::StepCreated,
            JournalKind::StepStarted,
            JournalKind::StepFinished,
            JournalKind::ExecutionTransition,
            JournalKind::IncidentFolded,
            JournalKind::PolicyPublished,
        ] {
            assert_eq!(JournalKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(JournalKind::parse("incident.status"), None);
    }

    #[test]
    fn timeline_journal_mirrors_the_entry_exactly() {
        let entry = IncidentTimelineEntry {
            incident: IncidentId::from_uuid(uuid::Uuid::from_u128(3)),
            at: jiff::Timestamp::from_millisecond(5_000).unwrap(),
            kind: "status.changed".to_owned(),
            summary: "opened".to_owned(),
            detail: json!({"from": "pending"}),
        };
        let draft = timeline_journal(&entry);
        assert_eq!(draft.aggregate, AggregateRef::incident(entry.incident));
        assert_eq!(draft.kind, JournalKind::IncidentTimeline);
        assert_eq!(draft.payload_v, 1);
        assert_eq!(draft.actor, Actor::System(SYSTEM_INCIDENTS.to_owned()));
        assert_eq!(draft.occurred_at, entry.at);
        assert_eq!(
            draft.payload,
            json!({
                "kind": "status.changed",
                "summary": "opened",
                "detail": {"from": "pending"},
            })
        );
    }
}
