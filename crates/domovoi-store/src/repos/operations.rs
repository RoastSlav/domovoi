//! Reading operations. Writing them is the unit of work's job: see `uow/operations.rs`.

use async_trait::async_trait;
use domovoi_core::ports::{OperationFilter, OperationRepo, Page, Paged, StoreError, Versioned};
use domovoi_core::{
    ContractId, ExecutionId, ExecutionRecord, HostId, IncidentId, Operation, OperationId,
    OperationSnapshot,
};
use jiff::Timestamp;

use super::operations_rows::{
    EdgeRow, ExecutionRow, OperationRow, StepRow, execution_columns, operation_columns,
    step_columns,
};
use crate::codec::{Cursor, fetch_limit, json, ms, paginate, parse};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl OperationRepo for Store {
    async fn get(&self, id: OperationId) -> Result<Option<Versioned<Operation>>, StoreError> {
        const SQL: &str = concat!(operation_columns!(), " WHERE id = ?");
        let row: Option<OperationRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(OperationRow::into_versioned).transpose()
    }

    async fn load(&self, id: OperationId) -> Result<Option<OperationSnapshot>, StoreError> {
        const OPERATION: &str = concat!(operation_columns!(), " WHERE id = ?");
        const STEPS: &str = concat!(step_columns!(), " WHERE operation_id = ? ORDER BY ordinal");
        const EDGES: &str = "SELECT operation_id, from_step, to_step, kind FROM step_edges \
            WHERE operation_id = ? ORDER BY rowid";
        const EXECUTIONS: &str = concat!(
            execution_columns!(),
            " WHERE operation_id = ? ORDER BY \
             (SELECT ordinal FROM steps WHERE steps.id = executions.execution_id)"
        );
        const INCIDENTS: &str = "SELECT incident_id FROM operation_incidents \
            WHERE operation_id = ? ORDER BY incident_id";
        let key = id.to_string();
        // One read transaction: every query below sees the same snapshot.
        let mut tx = self.read.begin().await.map_err(db)?;
        let row: Option<OperationRow> = sqlx::query_as(OPERATION)
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let steps: Vec<StepRow> = sqlx::query_as(STEPS)
            .bind(&key)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
        let edges: Vec<EdgeRow> = sqlx::query_as(EDGES)
            .bind(&key)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
        let executions: Vec<ExecutionRow> = sqlx::query_as(EXECUTIONS)
            .bind(&key)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
        let incidents: Vec<(String,)> = sqlx::query_as(INCIDENTS)
            .bind(&key)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)?;

        let Versioned {
            value: operation,
            version,
        } = row.into_versioned()?;
        Ok(Some(OperationSnapshot {
            operation,
            version,
            steps: steps
                .into_iter()
                .map(StepRow::into_record)
                .collect::<Result<_, _>>()?,
            edges: edges
                .into_iter()
                .map(EdgeRow::into_edge)
                .collect::<Result<_, _>>()?,
            executions: executions
                .into_iter()
                .map(ExecutionRow::into_versioned)
                .collect::<Result<_, _>>()?,
            incidents: incidents
                .into_iter()
                .map(|(incident,)| parse(&incident, "incident id"))
                .collect::<Result<_, _>>()?,
        }))
    }

    async fn list(
        &self,
        filter: &OperationFilter,
        page: &Page,
    ) -> Result<Paged<Operation>, StoreError> {
        const SQL: &str = concat!(
            operation_columns!(),
            " WHERE (?1 IS NULL OR status = ?1) \
            AND (?2 IS NULL OR EXISTS (SELECT 1 FROM operation_incidents i \
                 WHERE i.operation_id = operations.id AND i.incident_id = ?2)) \
            AND (?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(operations.pinned) p \
                 WHERE json_extract(p.value, '$.host') = ?3)) \
            AND (?4 IS NULL OR kind = ?4) \
            AND (?5 IS NULL OR created_at < ?5 OR (created_at = ?5 AND id < ?6)) \
            ORDER BY created_at DESC, id DESC LIMIT ?7"
        );
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<OperationRow> = sqlx::query_as(SQL)
            .bind(filter.status.as_deref())
            .bind(filter.incident.map(|i| i.to_string()))
            .bind(filter.host.map(|h| h.to_string()))
            .bind(filter.kind.map(|k| k.as_str()))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let operations = rows
            .into_iter()
            .map(OperationRow::into_versioned)
            .collect::<Result<Vec<_>, _>>()?;
        let (operations, next_cursor) = paginate(operations, limit, |row| Cursor {
            key: ms(row.value.created_at),
            id: row.value.id.to_string(),
        });
        Ok(Paged {
            items: operations.into_iter().map(|row| row.value).collect(),
            next_cursor,
        })
    }

    async fn for_incident(&self, incident: IncidentId) -> Result<Vec<Operation>, StoreError> {
        const SQL: &str = concat!(
            operation_columns!(),
            " WHERE id IN (SELECT operation_id FROM operation_incidents WHERE incident_id = ?) \
             ORDER BY created_at DESC, id DESC"
        );
        let rows: Vec<OperationRow> = sqlx::query_as(SQL)
            .bind(incident.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(OperationRow::into_operation).collect()
    }

    async fn execution(
        &self,
        id: ExecutionId,
    ) -> Result<Option<Versioned<ExecutionRecord>>, StoreError> {
        const SQL: &str = concat!(execution_columns!(), " WHERE execution_id = ?");
        let row: Option<ExecutionRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ExecutionRow::into_versioned).transpose()
    }

    async fn dispatched_on(&self, host: HostId) -> Result<Vec<ExecutionRecord>, StoreError> {
        const SQL: &str = concat!(
            execution_columns!(),
            " WHERE host_id = ? AND \
             (SELECT status FROM steps WHERE steps.id = executions.execution_id) = 'running' \
             ORDER BY created_at, execution_id"
        );
        let rows: Vec<ExecutionRow> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(ExecutionRow::into_record).collect()
    }

    async fn dispatched_since(
        &self,
        host: HostId,
        contract: &ContractId,
        since: Timestamp,
    ) -> Result<u32, StoreError> {
        const SQL: &str = "SELECT count(*) FROM executions \
            WHERE host_id = ? AND contract = ? AND dispatched_at >= ?";
        let (count,): (i64,) = sqlx::query_as(SQL)
            .bind(host.to_string())
            .bind(contract.as_str())
            .bind(ms(since))
            .fetch_one(&self.read)
            .await
            .map_err(db)?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    async fn dispatched_between(
        &self,
        host: HostId,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<ExecutionRecord>, StoreError> {
        const SQL: &str = concat!(
            execution_columns!(),
            " WHERE host_id = ? AND dispatched_at >= ? AND dispatched_at <= ? \
             ORDER BY dispatched_at, execution_id"
        );
        let rows: Vec<ExecutionRow> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .bind(ms(from))
            .bind(ms(to))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(ExecutionRow::into_record).collect()
    }

    async fn with_status(
        &self,
        statuses: &[&str],
        limit: u16,
    ) -> Result<Vec<Operation>, StoreError> {
        const SQL: &str = concat!(
            operation_columns!(),
            " WHERE status IN (SELECT value FROM json_each(?1)) \
             ORDER BY created_at, id LIMIT ?2"
        );
        let rows: Vec<OperationRow> = sqlx::query_as(SQL)
            .bind(json(statuses)?)
            .bind(i64::from(limit.clamp(1, Page::MAX_LIMIT)))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(OperationRow::into_operation).collect()
    }
}
