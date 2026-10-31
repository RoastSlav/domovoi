//! The append-only audit log (INV-7).

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

use super::{Page, Paged, StoreError};

/// One audited event.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEvent {
    /// When it happened.
    pub at: Timestamp,
    /// Who did it: `user:<id>`, `chat:<platform>:<user>` or `system:<component>`.
    pub actor: String,
    /// Dotted kind, for example `action.approved`.
    pub kind: String,
    /// What it was done to, for example `action:<id>`.
    pub subject: String,
    /// Structured detail; `Value::Null` when there is none.
    pub detail: Value,
}

/// Which audit events a listing returns. Set fields must all match exactly; the default
/// returns everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    /// Only this actor.
    pub actor: Option<String>,
    /// Only this kind.
    pub kind: Option<String>,
    /// Only this subject.
    pub subject: Option<String>,
    /// Only events at or after this instant.
    pub since: Option<Timestamp>,
}

/// The audit log. There is deliberately no way to change or delete an event; the SQLite
/// adapter also rejects `UPDATE` and `DELETE` with a trigger.
#[async_trait]
pub trait AuditLog: Send + Sync {
    /// Appends an event and returns its sequence number. Sequence numbers start at 1 and
    /// grow by one with every append.
    async fn append(&self, event: &AuditEvent) -> Result<u64, StoreError>;

    /// One page of events matching `filter`, highest sequence number (newest) first.
    async fn list(
        &self,
        filter: &AuditFilter,
        page: &Page,
    ) -> Result<Paged<AuditEvent>, StoreError>;
}
