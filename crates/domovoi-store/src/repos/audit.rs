//! The audit log: append and list only. The database rejects every `UPDATE` and `DELETE`
//! on `audit_log` with a trigger (INV-7).

use async_trait::async_trait;
use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, Page, Paged, StoreError};

use crate::codec::{Cursor, fetch_limit, from_json, from_ms, json, ms, paginate, uint};
use crate::db::Store;
use crate::error::db;

#[derive(sqlx::FromRow)]
struct AuditRow {
    seq: i64,
    at: i64,
    actor: String,
    kind: String,
    subject: String,
    detail: String,
}

#[async_trait]
impl AuditLog for Store {
    async fn append(&self, event: &AuditEvent) -> Result<u64, StoreError> {
        const SQL: &str = "INSERT INTO audit_log (at, actor, kind, subject, detail) \
            VALUES (?, ?, ?, ?, ?) RETURNING seq";
        let (seq,): (i64,) = sqlx::query_as(SQL)
            .bind(ms(event.at))
            .bind(&event.actor)
            .bind(&event.kind)
            .bind(&event.subject)
            .bind(json(&event.detail)?)
            .fetch_one(&self.write)
            .await
            .map_err(db)?;
        Ok(uint(seq))
    }

    async fn list(
        &self,
        filter: &AuditFilter,
        page: &Page,
    ) -> Result<Paged<AuditEvent>, StoreError> {
        const SQL: &str = "SELECT seq, at, actor, kind, subject, detail FROM audit_log \
            WHERE (?1 IS NULL OR actor = ?1) AND (?2 IS NULL OR kind = ?2) \
              AND (?3 IS NULL OR subject = ?3) AND (?4 IS NULL OR at >= ?4) \
              AND (?5 IS NULL OR seq < ?5) \
            ORDER BY seq DESC LIMIT ?6";
        let (after, _) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<AuditRow> = sqlx::query_as(SQL)
            .bind(filter.actor.as_deref())
            .bind(filter.kind.as_deref())
            .bind(filter.subject.as_deref())
            .bind(filter.since.map(ms))
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
            .map(|r| {
                Ok(AuditEvent {
                    at: from_ms(r.at)?,
                    actor: r.actor,
                    kind: r.kind,
                    subject: r.subject,
                    detail: from_json(&r.detail, "audit detail")?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
        Ok(Paged { items, next_cursor })
    }
}
