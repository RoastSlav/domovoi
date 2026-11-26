//! Read-only access to the audit log with its hash-chain fields, for the audit page.

use async_trait::async_trait;

use super::{AuditFilter, Page, Paged, StoreError};
use crate::audit_chain::{AuditAnchor, ChainRow, Hash, VerifyReport, VerifyScope};

/// Which audit rows a listing returns: the event filters of `AuditLog::list`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRowFilter {
    /// The event filters shared with `AuditLog::list`.
    pub events: AuditFilter,
}

/// Read-only access to the audit log with its hash-chain fields, for the audit page. Nothing
/// here writes.
#[async_trait]
pub trait AuditInspect: Send + Sync {
    /// One page of rows matching `filter`, highest sequence number first, with `prev_hash` and
    /// `hash`.
    async fn rows(
        &self,
        filter: &AuditRowFilter,
        page: &Page,
    ) -> Result<Paged<ChainRow>, StoreError>;
    /// The chain's anchor; `None` only if the store was not opened through
    /// `Store::open`.
    async fn anchor(&self) -> Result<Option<AuditAnchor>, StoreError>;
    /// The newest row's sequence number and hash, `None` before the first row.
    async fn head(&self) -> Result<Option<(u64, Hash)>, StoreError>;
    /// Verifies the history as `Store::verify_audit` does; an unreadable or edited anchor is an
    /// error, never a pass.
    async fn verify(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError>;
}
