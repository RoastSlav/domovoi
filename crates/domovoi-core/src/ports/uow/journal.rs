//! The operational journal: what happened, in commit order, and the audit entries that share the
//! same transaction.

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

use crate::ports::repos::{AuditEvent, IncidentTimelineEntry, StoreError};

use super::AggregateRef;

/// Typed journal kinds. Closed on purpose; later releases add variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalKind {
    /// A line in an incident's timeline.
    IncidentTimeline,
    /// An action changed status.
    ActionTransition,
}

impl JournalKind {
    /// Stable dotted name stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IncidentTimeline => "incident.timeline",
            Self::ActionTransition => "action.transition",
        }
    }

    /// Reads [`JournalKind::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "incident.timeline" => Some(Self::IncidentTimeline),
            "action.transition" => Some(Self::ActionTransition),
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
    /// Today's actor string (`user:<id>` or `system:<component>`).
    pub actor: String,
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
    /// The event to append.
    pub event: AuditEvent,
}

impl AuditDraft {
    /// Wraps an audit event for a transaction.
    #[must_use]
    pub fn new(event: AuditEvent) -> Self {
        Self { event }
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
        actor: "system:incidents".to_owned(),
        occurred_at: entry.at,
    }
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

    /// The highest assigned `seq`, 0 when the journal is empty.
    async fn head_seq(&self) -> Result<u64, StoreError>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::id::IncidentId;

    use super::*;

    #[test]
    fn journal_kinds_round_trip() {
        for kind in [JournalKind::IncidentTimeline, JournalKind::ActionTransition] {
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
        assert_eq!(draft.actor, "system:incidents");
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
