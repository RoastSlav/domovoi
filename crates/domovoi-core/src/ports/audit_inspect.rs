//! Read-only access to the audit log with its hash-chain fields, for the audit page.

use async_trait::async_trait;

use super::{AuditFilter, Page, Paged, StoreError};
use crate::audit_chain::{AuditAnchor, ChainRow, Hash, VerifyReport, VerifyScope};

/// Which part of the audit history a listing covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditHistory {
    /// Rows written before the hash chain existed: `seq <= audit_anchor.legacy_last_id`.
    Legacy,
    /// Rows written since: `seq > audit_anchor.legacy_last_id`.
    Chained,
}

/// Which audit rows a listing returns: the event filters of `AuditLog::list`, optionally one
/// part of the history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRowFilter {
    /// The event filters shared with `AuditLog::list`.
    pub events: AuditFilter,
    /// Restricts the listing to one part of the history.
    pub history: Option<AuditHistory>,
}

/// Read-only access to the audit log with its hash-chain fields, for the audit page. Nothing
/// here writes.
#[async_trait]
pub trait AuditInspect: Send + Sync {
    /// One page of rows matching `filter`, highest sequence number first, with `prev_hash` and
    /// `hash` (both `None` on a legacy row).
    async fn rows(
        &self,
        filter: &AuditRowFilter,
        page: &Page,
    ) -> Result<Paged<ChainRow>, StoreError>;
    /// The anchor over the legacy history; `None` only if the store was not opened through
    /// `Store::open`.
    async fn anchor(&self) -> Result<Option<AuditAnchor>, StoreError>;
    /// The newest chained row's sequence number and hash, `None` before the first chained row.
    async fn head(&self) -> Result<Option<(u64, Hash)>, StoreError>;
    /// Verifies the history as `Store::verify_audit` does; an unreadable or edited anchor is an
    /// error, never a pass.
    async fn verify(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError>;
}
