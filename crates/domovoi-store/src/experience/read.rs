//! `ExperienceRead` on the read pool.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::OperationId;
use domovoi_core::experience::{
    ExperienceHit, ExperienceQuery, ResolutionBrief, ResolutionOutcome, ResolutionSummary,
    SimilarHit, SimilarQuery, SummaryRevision,
};
use domovoi_core::ports::{ExperienceRead, StoreError};

use super::rows::{RevisionRow, current_sql, field_order, load_summaries};
use super::{search, similar};
use crate::codec::{from_ms, parse};
use crate::db::Store;
use crate::error::{corrupt, db};

#[async_trait]
impl ExperienceRead for Store {
    async fn summary(
        &self,
        operation: OperationId,
    ) -> Result<Option<ResolutionSummary>, StoreError> {
        Ok(load_summaries(&self.read, &[operation])
            .await?
            .remove(&operation))
    }

    async fn revisions(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        let sql = format!(
            "SELECT operation_id, field, revision, value, source, actor, created_at \
             FROM resolution_summary_fields WHERE operation_id = ? \
             ORDER BY CASE field {} END, revision",
            field_order()
        );
        let rows: Vec<RevisionRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(operation.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(RevisionRow::into_revision).collect()
    }

    async fn current(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        let rows: Vec<RevisionRow> =
            sqlx::query_as(sqlx::AssertSqlSafe(current_sql("f.operation_id = ?")))
                .bind(operation.to_string())
                .fetch_all(&self.read)
                .await
                .map_err(db)?;
        rows.into_iter().map(RevisionRow::into_revision).collect()
    }

    async fn briefs(
        &self,
        operations: &[OperationId],
    ) -> Result<BTreeMap<OperationId, ResolutionBrief>, StoreError> {
        let mut out = BTreeMap::new();
        if operations.is_empty() {
            return Ok(out);
        }
        let marks = vec!["?"; operations.len()].join(", ");
        let sql = format!(
            "SELECT operation_id, outcome, verified, ended_at FROM operation_resolutions \
             WHERE operation_id IN ({marks})"
        );
        let mut query = sqlx::query_as::<_, (String, String, i64, i64)>(sqlx::AssertSqlSafe(sql));
        for id in operations {
            query = query.bind(id.to_string());
        }
        for (id, outcome, verified, ended) in query.fetch_all(&self.read).await.map_err(db)? {
            out.insert(
                parse(&id, "operation id")?,
                ResolutionBrief {
                    outcome: ResolutionOutcome::parse(&outcome)
                        .ok_or_else(|| corrupt("summary outcome", &outcome))?,
                    verified: verified != 0,
                    ended_at: from_ms(ended)?,
                },
            );
        }
        Ok(out)
    }

    async fn search(&self, query: &ExperienceQuery) -> Result<Vec<ExperienceHit>, StoreError> {
        search::search(self, query).await
    }

    async fn similar(&self, query: &SimilarQuery) -> Result<Vec<SimilarHit>, StoreError> {
        similar::similar(self, query).await
    }

    async fn undrafted(&self, limit: u16) -> Result<Vec<OperationId>, StoreError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT o.id FROM operations o \
             WHERE o.status IN ('succeeded', 'failed', 'escalated', 'cancelled') \
             AND NOT EXISTS (SELECT 1 FROM resolution_summary_fields f WHERE f.operation_id = o.id) \
             ORDER BY o.ended_at, o.id LIMIT ?",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.read)
        .await
        .map_err(db)?;
        rows.into_iter()
            .map(|(id,)| parse(&id, "operation id"))
            .collect()
    }
}
