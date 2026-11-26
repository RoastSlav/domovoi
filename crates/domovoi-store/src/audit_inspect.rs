//! `AuditInspect` on SQLite: audit rows with their chain fields, read on the read pool.

use async_trait::async_trait;
use domovoi_core::audit_chain::{AuditAnchor, ChainRow, Hash, VerifyReport, VerifyScope};
use domovoi_core::ports::{AuditInspect, AuditRowFilter, Page, Paged, StoreError};

use crate::audit_chain::AuditChainRow;
use crate::codec::{Cursor, fetch_limit, ms, paginate};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl AuditInspect for Store {
    async fn rows(
        &self,
        filter: &AuditRowFilter,
        page: &Page,
    ) -> Result<Paged<ChainRow>, StoreError> {
        const SQL: &str = "SELECT seq, at, actor, kind, subject, detail, prev_hash, hash \
            FROM audit_log \
            WHERE (?1 IS NULL OR actor = ?1) AND (?2 IS NULL OR kind = ?2) \
              AND (?3 IS NULL OR subject = ?3) AND (?4 IS NULL OR at >= ?4) \
              AND (?5 IS NULL OR seq < ?5) \
            ORDER BY seq DESC LIMIT ?6";
        let (after, _) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let events = &filter.events;
        let rows: Vec<AuditChainRow> = sqlx::query_as(SQL)
            .bind(events.actor.as_deref())
            .bind(events.kind.as_deref())
            .bind(events.subject.as_deref())
            .bind(events.since.map(ms))
            .bind(after)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let (rows, next_cursor) = paginate(rows, limit, |r| Cursor {
            key: r.seq,
            id: String::new(),
        });
        let items = rows
            .into_iter()
            .map(AuditChainRow::into_chain_row)
            .collect::<Result<_, StoreError>>()?;
        Ok(Paged { items, next_cursor })
    }

    async fn anchor(&self) -> Result<Option<AuditAnchor>, StoreError> {
        self.audit_anchor().await
    }

    async fn head(&self) -> Result<Option<(u64, Hash)>, StoreError> {
        self.audit_head().await
    }

    async fn verify(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError> {
        self.verify_audit(scope).await
    }
}
