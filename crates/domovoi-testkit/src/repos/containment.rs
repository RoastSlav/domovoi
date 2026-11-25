//! The in-memory twin of `containment_blocks`, with the same guards and uniqueness rules as
//! SQLite.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateRef, BlockPhase, BlockRow, CommitError, ContainmentRepo, ContainmentWrite,
    Expectation, Observed, StoreError,
};
use domovoi_core::{ExecutionId, HostId, OperationId, StepId};

use super::{InMemoryStore, State};

/// Blocks by the execution id of their `apply`.
#[derive(Clone, Default)]
pub(super) struct ContainmentState {
    blocks: BTreeMap<ExecutionId, BlockRow>,
}

fn conflict(block: ExecutionId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::containment_block(block),
        expected,
        found,
    }
}

/// Whether another block already uses `execution` as its confirm execution (`confirm`) or
/// its rollback execution; the two columns are unique separately, as in SQLite.
fn execution_taken(
    state: &ContainmentState,
    owner: ExecutionId,
    execution: ExecutionId,
    confirm: bool,
) -> bool {
    state.blocks.iter().any(|(id, row)| {
        *id != owner
            && if confirm {
                row.confirm_execution == Some(execution)
            } else {
                row.rollback_execution == Some(execution)
            }
    })
}

fn check_unique(
    state: &ContainmentState,
    owner: ExecutionId,
    confirm: Option<ExecutionId>,
    rollback: Option<ExecutionId>,
) -> Result<(), CommitError> {
    let taken = confirm.is_some_and(|id| execution_taken(state, owner, id, true))
        || rollback.is_some_and(|id| execution_taken(state, owner, id, false));
    if taken {
        return Err(CommitError::Constraint(
            "UNIQUE constraint failed: containment_blocks execution".to_owned(),
        ));
    }
    Ok(())
}

/// Stores one containment write.
pub(super) fn apply(state: &mut State, write: &ContainmentWrite) -> Result<(), CommitError> {
    let blocks = &mut state.containment;
    match write {
        ContainmentWrite::Create(row) => {
            if let Some(stored) = blocks.blocks.get(&row.block_id) {
                return Err(conflict(
                    row.block_id,
                    Expectation::New,
                    Observed::Version(stored.version),
                ));
            }
            check_unique(
                blocks,
                row.block_id,
                row.confirm_execution,
                row.rollback_execution,
            )?;
            let mut row = row.clone();
            row.version = 1;
            blocks.blocks.insert(row.block_id, row);
        }
        ContainmentWrite::Phase {
            block,
            expected,
            update,
        } => {
            let guard = Expectation::Version(expected.version);
            let Some(stored) = blocks.blocks.get(block) else {
                return Err(conflict(*block, guard, Observed::Missing));
            };
            if stored.phase != expected.phase || stored.version != expected.version {
                return Err(conflict(*block, guard, Observed::Version(stored.version)));
            }
            check_unique(
                blocks,
                *block,
                update.confirm_execution,
                update.rollback_execution,
            )?;
            if let Some(row) = blocks.blocks.get_mut(block) {
                row.phase = update.phase;
                row.confirm_execution = update.confirm_execution.or(row.confirm_execution);
                row.rollback_execution = update.rollback_execution.or(row.rollback_execution);
                row.applied_at = update.applied_at.or(row.applied_at);
                row.deadline = update.deadline.or(row.deadline);
                row.existing_flows = update.existing_flows.clone().or(row.existing_flows.take());
                row.bridge_gap = update.bridge_gap.or(row.bridge_gap);
                row.failure_token = update.failure_token.clone().or(row.failure_token.take());
                row.failure_message = update
                    .failure_message
                    .clone()
                    .or(row.failure_message.take());
                row.updated_at = update.at;
                row.version += 1;
            }
        }
    }
    Ok(())
}

fn newest_first(rows: &mut [BlockRow]) {
    rows.sort_by(|a, b| {
        (b.created_at, b.block_id.to_string()).cmp(&(a.created_at, a.block_id.to_string()))
    });
}

#[async_trait]
impl ContainmentRepo for InMemoryStore {
    async fn get_block(&self, block: ExecutionId) -> Result<Option<BlockRow>, StoreError> {
        Ok(self.begin()?.containment.blocks.get(&block).cloned())
    }

    async fn block_for_step(
        &self,
        operation: OperationId,
        step: StepId,
    ) -> Result<Option<BlockRow>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<BlockRow> = state
            .containment
            .blocks
            .values()
            .filter(|row| row.operation_id == operation && row.step_id == step)
            .cloned()
            .collect();
        newest_first(&mut rows);
        Ok(rows.into_iter().next())
    }

    async fn blocks_for_host(&self, host: HostId, limit: u32) -> Result<Vec<BlockRow>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<BlockRow> = state
            .containment
            .blocks
            .values()
            .filter(|row| row.host_id == host)
            .cloned()
            .collect();
        newest_first(&mut rows);
        rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(rows)
    }

    async fn reconcilable_blocks(&self, host: HostId) -> Result<Vec<BlockRow>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<BlockRow> = state
            .containment
            .blocks
            .values()
            .filter(|row| row.host_id == host && BlockPhase::RECONCILABLE.contains(&row.phase))
            .cloned()
            .collect();
        rows.sort_by(|a, b| {
            (a.created_at, a.block_id.to_string()).cmp(&(b.created_at, b.block_id.to_string()))
        });
        Ok(rows)
    }
}
