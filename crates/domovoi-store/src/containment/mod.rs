//! Containment blocks: the row codec, write application and retention. The read port is
//! implemented in `repos/containment.rs`.

mod apply;

use domovoi_core::ports::{BlockPhase, BlockRow, StoreError};
use jiff::{SignedDuration, Timestamp};

pub(crate) use apply::apply;

use crate::codec::{from_ms, from_ms_opt, ms, parse, uint};
use crate::db::Store;
use crate::error::{corrupt, db};

/// Columns of `containment_blocks`, in [`Row`] order.
macro_rules! columns {
    () => {
        "SELECT block_id, host_id, operation_id, step_id, source, ttl_s, plan_hash, phase, \
    authorization_kind, assurance, confirm_execution, rollback_execution, applied_at, \
    deadline, existing_flows, bridge_gap, failure_token, failure_message, created_at, \
    updated_at, version FROM containment_blocks"
    };
}
pub(crate) use columns;

/// One `containment_blocks` row.
#[derive(sqlx::FromRow)]
pub(crate) struct Row {
    block_id: String,
    host_id: String,
    operation_id: String,
    step_id: String,
    source: String,
    ttl_s: i64,
    plan_hash: String,
    phase: String,
    authorization_kind: String,
    assurance: String,
    confirm_execution: Option<String>,
    rollback_execution: Option<String>,
    applied_at: Option<i64>,
    deadline: Option<i64>,
    existing_flows: Option<String>,
    bridge_gap: Option<i64>,
    failure_token: Option<String>,
    failure_message: Option<String>,
    created_at: i64,
    updated_at: i64,
    version: i64,
}

impl Row {
    /// The domain block.
    pub(crate) fn into_block(self) -> Result<BlockRow, StoreError> {
        Ok(BlockRow {
            block_id: parse(&self.block_id, "block id")?,
            host_id: parse(&self.host_id, "host id")?,
            operation_id: parse(&self.operation_id, "operation id")?,
            step_id: parse(&self.step_id, "step id")?,
            source: self.source,
            ttl_s: u32::try_from(self.ttl_s).map_err(|e| corrupt("block ttl", e))?,
            plan_hash: self.plan_hash,
            phase: BlockPhase::parse(&self.phase)
                .ok_or_else(|| corrupt("block phase", &self.phase))?,
            authorization_kind: self.authorization_kind,
            assurance: self.assurance,
            confirm_execution: self
                .confirm_execution
                .as_deref()
                .map(|id| parse(id, "confirm execution id"))
                .transpose()?,
            rollback_execution: self
                .rollback_execution
                .as_deref()
                .map(|id| parse(id, "rollback execution id"))
                .transpose()?,
            applied_at: from_ms_opt(self.applied_at)?,
            deadline: from_ms_opt(self.deadline)?,
            existing_flows: self.existing_flows,
            bridge_gap: self.bridge_gap.map(|value| value != 0),
            failure_token: self.failure_token,
            failure_message: self.failure_message,
            created_at: from_ms(self.created_at)?,
            updated_at: from_ms(self.updated_at)?,
            version: uint(self.version),
        })
    }
}

/// Deletes blocks that reached an end state and were last changed before the cutoff. A block
/// still in flight, confirmed or with an unknown outcome is never deleted; the audit log is
/// untouched.
const DELETE_OLD_BLOCKS: &str = "DELETE FROM containment_blocks WHERE updated_at < ? \
    AND phase IN ('rolled_back', 'expired', 'lost', 'not_run', 'failed')";

impl Store {
    /// Deletes containment blocks in an end state whose last change is older than `keep` at
    /// `now`. Returns how many rows were deleted.
    pub async fn purge_containment_blocks(
        &self,
        now: Timestamp,
        keep: SignedDuration,
    ) -> Result<u64, StoreError> {
        let before = ms(now.checked_sub(keep).unwrap_or(Timestamp::MIN));
        let deleted = sqlx::query(DELETE_OLD_BLOCKS)
            .bind(before)
            .execute(&self.write)
            .await
            .map_err(db)?
            .rows_affected();
        tracing::info!(deleted, "purged old containment blocks");
        Ok(deleted)
    }
}
