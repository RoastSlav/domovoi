//! Helpers of the commit path that read state and build parts of a transaction.

use domovoi_core::ports::{AggregateRef, EffectDraft, EffectKind, EffectStatus, Transaction};
use domovoi_core::{
    EffectId, ExecutionId, ExecutionRecord, ExecutionState, OperationSnapshot, StepKind,
    StepOutcome,
};
use jiff::Timestamp;

use super::OpShared;
use super::commands::Held;
use crate::operations::decide::{RECONCILE_ATTEMPTS, step_payload};
use crate::operations::error::OperationError;
use crate::operations::state::OperationState;

/// Finds the executions that left "awaiting approval" between `loaded` and `after`, and
/// clears the chat token of every execution that is no longer awaiting approval by adding its
/// cleared row to `updated` (unless that row is already being written).
pub(super) fn spend_tokens(
    loaded: &OperationSnapshot,
    after: &OperationState,
    updated: &mut Vec<(ExecutionRecord, u64)>,
) -> Vec<ExecutionId> {
    let before = OperationState::new(loaded.clone());
    let awaiting = |state: &OperationState, step| {
        state.execution_state(step) == Some(ExecutionState::AwaitingApproval)
    };
    let mut left = Vec::new();
    for row in &after.snapshot().executions {
        let step = row.value.step;
        let now_awaiting = awaiting(after, step);
        if before.execution_of(step).is_some() && awaiting(&before, step) && !now_awaiting {
            left.push(row.value.execution);
        }
        let spent = row.value.chat_token_hash().is_some() && !now_awaiting;
        if spent && !updated.iter().any(|(record, _)| record.step == step) {
            let mut cleared = row.value.clone();
            cleared.clear_chat_token_hash();
            let version = loaded.execution_of(step).map_or(0, |r| r.version);
            updated.push((cleared, version));
        }
    }
    left
}

/// Refuses a stale claim: the effect must still be claimed under exactly this token.
pub(super) async fn check_claim(shared: &OpShared, held: Held) -> Result<(), OperationError> {
    let record = shared
        .deps
        .effect_read
        .effect(held.effect)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?;
    let current = record.is_some_and(|record| {
        record.status == EffectStatus::Claimed.as_str() && record.claim_token == Some(held.claim)
    });
    if current {
        Ok(())
    } else {
        Err(OperationError::StaleEffect)
    }
}

/// After an operation ended, a run still going is not recalled: its result must still be
/// reconciled, so a fresh reconcile is queued for it (the cancellation just dropped the
/// pending one). It carries no dedup key, because a claimed one may still be in flight and
/// a second delivery is harmless.
pub(super) fn keep_reconciling(
    mut tx: Transaction,
    after: &OperationState,
    now: Timestamp,
) -> Transaction {
    let operation = after.operation().id;
    for record in &after.snapshot().steps {
        if record.step.kind == StepKind::Execution
            && record.progress.outcome == StepOutcome::Running
        {
            tx = tx.effect(EffectDraft {
                id: EffectId::new(),
                kind: EffectKind::Reconcile,
                aggregate: AggregateRef::operation(operation),
                payload: step_payload(operation, record.step.id),
                not_before: now,
                max_attempts: RECONCILE_ATTEMPTS,
                dedup_key: None,
                created_at: now,
            });
        }
    }
    tx
}
