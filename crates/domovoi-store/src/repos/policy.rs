//! Reading policy versions and shadow decisions. Writing them is the unit of work's job: see
//! `policy/apply.rs`.

use async_trait::async_trait;
use domovoi_core::policy_store::{
    AckRecord, PolicyVersionRecord, ShadowCandidate, ShadowFilter, ShadowRecord, ShadowState,
    ShadowSummary, StoredRule,
};
use domovoi_core::ports::{Page, Paged, PolicyRepo, StoreError};
use domovoi_core::{ExecutionId, PolicyVersionId};
use jiff::Timestamp;

use crate::codec::{Cursor, fetch_limit, paginate};
use crate::db::Store;
use crate::error::db;
use crate::policy::queries;
use crate::policy::rows::{
    AckRow, CandidateRow, RuleRow, ShadowRow, StateRow, VersionRow, shadow_columns, version_columns,
};

#[async_trait]
impl PolicyRepo for Store {
    async fn active(&self) -> Result<Option<PolicyVersionRecord>, StoreError> {
        const SQL: &str = concat!(
            version_columns!(),
            " WHERE status = 'published' ORDER BY number DESC LIMIT 1"
        );
        let row: Option<VersionRow> = sqlx::query_as(SQL)
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(VersionRow::into_record).transpose()
    }

    async fn version(&self, number: u32) -> Result<Option<PolicyVersionRecord>, StoreError> {
        const SQL: &str = concat!(version_columns!(), " WHERE number = ?");
        let row: Option<VersionRow> = sqlx::query_as(SQL)
            .bind(i64::from(number))
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(VersionRow::into_record).transpose()
    }

    async fn by_id(&self, id: PolicyVersionId) -> Result<Option<PolicyVersionRecord>, StoreError> {
        const SQL: &str = concat!(version_columns!(), " WHERE id = ?");
        let row: Option<VersionRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(VersionRow::into_record).transpose()
    }

    async fn versions(&self, page: &Page) -> Result<Paged<PolicyVersionRecord>, StoreError> {
        const SQL: &str = concat!(
            version_columns!(),
            " WHERE status = 'published' AND (?1 IS NULL OR number < ?1) \
             ORDER BY number DESC LIMIT ?2"
        );
        let (after, _) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<VersionRow> = sqlx::query_as(SQL)
            .bind(after)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let records = rows
            .into_iter()
            .map(VersionRow::into_record)
            .collect::<Result<Vec<_>, _>>()?;
        let (items, next_cursor) = paginate(records, limit, |record| Cursor {
            key: record.number.map_or(0, i64::from),
            id: String::new(),
        });
        Ok(Paged { items, next_cursor })
    }

    async fn draft(&self) -> Result<Option<PolicyVersionRecord>, StoreError> {
        const SQL: &str = concat!(version_columns!(), " WHERE status = 'draft'");
        let row: Option<VersionRow> = sqlx::query_as(SQL)
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(VersionRow::into_record).transpose()
    }

    async fn rules(&self, id: PolicyVersionId) -> Result<Vec<StoredRule>, StoreError> {
        let rows: Vec<RuleRow> = sqlx::query_as(
            "SELECT rule_id, ordinal, enabled, shadow, locked, body FROM policy_rules \
             WHERE version_id = ? ORDER BY ordinal",
        )
        .bind(id.to_string())
        .fetch_all(&self.read)
        .await
        .map_err(db)?;
        rows.into_iter().map(RuleRow::into_rule).collect()
    }

    async fn acknowledgements(&self, id: PolicyVersionId) -> Result<Vec<AckRecord>, StoreError> {
        let rows: Vec<AckRow> = sqlx::query_as(
            "SELECT finding_key, kind, first_rule, second_rule, resolution, note, \
             acknowledged_by, acknowledged_at FROM policy_acknowledgements \
             WHERE version_id = ? ORDER BY finding_key",
        )
        .bind(id.to_string())
        .fetch_all(&self.read)
        .await
        .map_err(db)?;
        rows.into_iter().map(AckRow::into_ack).collect()
    }

    async fn shadow_candidates(&self, limit: u16) -> Result<Vec<ShadowCandidate>, StoreError> {
        let rows: Vec<CandidateRow> = sqlx::query_as(
            "SELECT e.execution_id, e.operation_id, e.host_id, e.contract, e.contract_version, \
             e.check_key, e.target, e.args, o.pinned, e.created_at \
             FROM executions e JOIN operations o ON o.id = e.operation_id \
             LEFT JOIN policy_shadow_decisions d ON d.execution_id = e.execution_id \
             WHERE d.execution_id IS NULL \
             ORDER BY e.created_at, e.execution_id LIMIT ?",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.read)
        .await
        .map_err(db)?;
        rows.into_iter().map(CandidateRow::into_candidate).collect()
    }

    async fn shadow(&self, execution: ExecutionId) -> Result<Option<ShadowRecord>, StoreError> {
        const SQL: &str = concat!(shadow_columns!(), " WHERE execution_id = ?");
        let row: Option<ShadowRow> = sqlx::query_as(SQL)
            .bind(execution.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ShadowRow::into_record).transpose()
    }

    async fn shadow_records(
        &self,
        filter: &ShadowFilter,
        page: &Page,
    ) -> Result<Paged<ShadowRecord>, StoreError> {
        queries::shadow_records(&self.read, filter, page).await
    }

    async fn shadow_summary(&self, since: Timestamp) -> Result<ShadowSummary, StoreError> {
        queries::shadow_summary(&self.read, since).await
    }

    async fn shadow_state(&self) -> Result<ShadowState, StoreError> {
        let row: StateRow = sqlx::query_as(
            "SELECT last_sweep_at, last_error, total_evaluated, total_unevaluable, \
             total_skipped FROM policy_shadow_state WHERE id = 1",
        )
        .fetch_one(&self.read)
        .await
        .map_err(db)?;
        row.into_state()
    }
}
