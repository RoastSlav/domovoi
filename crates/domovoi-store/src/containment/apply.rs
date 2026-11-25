//! Applying one [`ContainmentWrite`] on one SQLite connection.

use domovoi_core::ExecutionId;
use domovoi_core::ports::{
    AggregateRef, BlockRow, CommitError, ContainmentWrite, Expectation, Observed, PhaseGuard,
    PhaseUpdate,
};
use sqlx::SqliteConnection;

use crate::codec::{int, ms};
use crate::uow::backend;

/// Applies one containment write.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &ContainmentWrite,
) -> Result<(), CommitError> {
    match write {
        ContainmentWrite::Create(block) => create(conn, block).await,
        ContainmentWrite::Phase {
            block,
            expected,
            update,
        } => phase(conn, *block, *expected, update).await,
    }
}

fn conflict(block: ExecutionId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::containment_block(block),
        expected,
        found,
    }
}

/// The stored phase word and version of a block.
async fn stored(
    conn: &mut SqliteConnection,
    block: ExecutionId,
) -> Result<Option<(String, i64)>, CommitError> {
    sqlx::query_as("SELECT phase, version FROM containment_blocks WHERE block_id = ?")
        .bind(block.to_string())
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)
}

async fn create(conn: &mut SqliteConnection, block: &BlockRow) -> Result<(), CommitError> {
    if let Some((_, version)) = stored(conn, block.block_id).await? {
        return Err(conflict(
            block.block_id,
            Expectation::New,
            Observed::Version(u64::try_from(version).unwrap_or(0)),
        ));
    }
    sqlx::query(
        "INSERT INTO containment_blocks (block_id, host_id, operation_id, step_id, source, \
         ttl_s, plan_hash, phase, authorization_kind, assurance, confirm_execution, \
         rollback_execution, applied_at, deadline, existing_flows, bridge_gap, failure_token, \
         failure_message, created_at, updated_at, version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(block.block_id.to_string())
    .bind(block.host_id.to_string())
    .bind(block.operation_id.to_string())
    .bind(block.step_id.to_string())
    .bind(&block.source)
    .bind(i64::from(block.ttl_s))
    .bind(&block.plan_hash)
    .bind(block.phase.as_str())
    .bind(&block.authorization_kind)
    .bind(&block.assurance)
    .bind(block.confirm_execution.map(|id| id.to_string()))
    .bind(block.rollback_execution.map(|id| id.to_string()))
    .bind(block.applied_at.map(ms))
    .bind(block.deadline.map(ms))
    .bind(&block.existing_flows)
    .bind(block.bridge_gap.map(i64::from))
    .bind(&block.failure_token)
    .bind(&block.failure_message)
    .bind(ms(block.created_at))
    .bind(ms(block.updated_at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}

async fn phase(
    conn: &mut SqliteConnection,
    block: ExecutionId,
    expected: PhaseGuard,
    update: &PhaseUpdate,
) -> Result<(), CommitError> {
    let guard = Expectation::Version(expected.version);
    let Some((phase, version)) = stored(conn, block).await? else {
        return Err(conflict(block, guard, Observed::Missing));
    };
    let found = u64::try_from(version).unwrap_or(0);
    if phase != expected.phase.as_str() || found != expected.version {
        return Err(conflict(block, guard, Observed::Version(found)));
    }
    sqlx::query(
        "UPDATE containment_blocks SET phase = ?, \
         confirm_execution = COALESCE(?, confirm_execution), \
         rollback_execution = COALESCE(?, rollback_execution), \
         applied_at = COALESCE(?, applied_at), deadline = COALESCE(?, deadline), \
         existing_flows = COALESCE(?, existing_flows), bridge_gap = COALESCE(?, bridge_gap), \
         failure_token = COALESCE(?, failure_token), \
         failure_message = COALESCE(?, failure_message), \
         updated_at = ?, version = version + 1 \
         WHERE block_id = ? AND version = ?",
    )
    .bind(update.phase.as_str())
    .bind(update.confirm_execution.map(|id| id.to_string()))
    .bind(update.rollback_execution.map(|id| id.to_string()))
    .bind(update.applied_at.map(ms))
    .bind(update.deadline.map(ms))
    .bind(&update.existing_flows)
    .bind(update.bridge_gap.map(i64::from))
    .bind(&update.failure_token)
    .bind(&update.failure_message)
    .bind(ms(update.at))
    .bind(block.to_string())
    .bind(int(expected.version))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}
