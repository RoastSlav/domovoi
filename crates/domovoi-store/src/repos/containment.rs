//! Reading containment blocks.

use async_trait::async_trait;
use domovoi_core::ports::{BlockPhase, BlockRow, ContainmentRepo, StoreError};
use domovoi_core::{ExecutionId, HostId, OperationId, StepId};

use crate::containment::{Row, columns};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl ContainmentRepo for Store {
    async fn get_block(&self, block: ExecutionId) -> Result<Option<BlockRow>, StoreError> {
        const SQL: &str = concat!(columns!(), " WHERE block_id = ?");
        let row: Option<Row> = sqlx::query_as(SQL)
            .bind(block.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(Row::into_block).transpose()
    }

    async fn block_for_step(
        &self,
        operation: OperationId,
        step: StepId,
    ) -> Result<Option<BlockRow>, StoreError> {
        const SQL: &str = concat!(
            columns!(),
            " WHERE operation_id = ? AND step_id = ? ORDER BY created_at DESC, block_id DESC \
             LIMIT 1"
        );
        let row: Option<Row> = sqlx::query_as(SQL)
            .bind(operation.to_string())
            .bind(step.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(Row::into_block).transpose()
    }

    async fn blocks_for_host(&self, host: HostId, limit: u32) -> Result<Vec<BlockRow>, StoreError> {
        const SQL: &str = concat!(
            columns!(),
            " WHERE host_id = ? ORDER BY created_at DESC, block_id DESC LIMIT ?"
        );
        let rows: Vec<Row> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .bind(i64::from(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(Row::into_block).collect()
    }

    async fn reconcilable_blocks(&self, host: HostId) -> Result<Vec<BlockRow>, StoreError> {
        const SQL: &str = concat!(
            columns!(),
            " WHERE host_id = ? AND phase IN (?, ?, ?, ?) ORDER BY created_at, block_id"
        );
        let [a, b, c, d] = BlockPhase::RECONCILABLE.map(BlockPhase::as_str);
        let rows: Vec<Row> = sqlx::query_as(SQL)
            .bind(host.to_string())
            .bind(a)
            .bind(b)
            .bind(c)
            .bind(d)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(Row::into_block).collect()
    }
}
