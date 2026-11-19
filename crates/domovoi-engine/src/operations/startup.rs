//! What the service does once before its runner claims anything.

use domovoi_core::ports::{
    AggregateKind, EffectKind, EffectReschedule, EffectStatus, StoreError, Transaction,
};

use crate::operations::writer::{OpShared, admit_queued};

/// The most claimed effects one start-up pass reads.
const CLAIMED_LIMIT: u16 = 500;

/// Returns every claimed `dispatch` effect of an operation to pending, then admits queued
/// operations once.
///
/// A dispatch effect is the one the runner never reclaims or reverts, because it may have
/// reached the agent. That is safe to undo for an operation's effect: the commit that records
/// the hand-off completes the effect in the same transaction, so a claimed one was never
/// handed over, and the writer's checks run again before it is.
pub(super) async fn recover(shared: &OpShared) {
    if let Err(err) = repend_dispatches(shared).await {
        tracing::error!(%err, "could not return claimed dispatch effects to pending");
    }
    admit_queued(shared).await;
}

/// One fenced reschedule per claimed operation dispatch effect, using its stored token.
async fn repend_dispatches(shared: &OpShared) -> Result<(), StoreError> {
    let claimed = shared
        .deps
        .effect_read
        .effects_with_status(EffectStatus::Claimed, CLAIMED_LIMIT)
        .await?;
    let now = shared.deps.clock.now();
    for effect in claimed {
        let ours = effect.kind == EffectKind::Dispatch.as_str()
            && effect.aggregate.kind == AggregateKind::Operation;
        let Some(token) = effect.claim_token.filter(|_| ours) else {
            continue;
        };
        let tx = Transaction::new().reschedule(EffectReschedule {
            id: effect.id,
            claim_token: token,
            not_before: now,
            error: "returned to pending at start".to_owned(),
            at: now,
        });
        if let Err(err) = shared.deps.uow.commit(tx).await {
            tracing::warn!(%err, effect = %effect.id, "a claimed dispatch effect was not returned");
        }
    }
    Ok(())
}
