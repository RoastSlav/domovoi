//! What the service does once before its runner claims anything.

use domovoi_core::ports::{AggregateKind, EffectKind, EffectReschedule, StoreError, Transaction};

use crate::operations::writer::{OpShared, admit_queued};

/// How many claimed dispatch effects one read returns.
const PAGE: u16 = 500;

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
    let queued = crate::operations::resolution::queue_missing(shared).await;
    if queued > 0 {
        tracing::info!(
            queued,
            "queued summaries for finished operations that had none"
        );
    }
    admit_queued(shared).await;
}

/// One fenced reschedule per claimed operation dispatch effect, using its stored token.
///
/// The store hands over the claimed dispatch effects of operations a page at a time, and a
/// returned effect leaves that set, so the loop reads again until a page brings nothing back
/// to pending (or the set is empty).
async fn repend_dispatches(shared: &OpShared) -> Result<(), StoreError> {
    let now = shared.deps.clock.now();
    loop {
        let claimed = shared
            .deps
            .effect_read
            .claimed_of(EffectKind::Dispatch, AggregateKind::Operation, PAGE)
            .await?;
        let page = claimed.len();
        let mut returned = 0_usize;
        for effect in claimed {
            let Some(token) = effect.claim_token else {
                continue;
            };
            let tx = Transaction::new().reschedule(EffectReschedule {
                id: effect.id,
                claim_token: token,
                not_before: now,
                error: "returned to pending at start".to_owned(),
                at: now,
            });
            match shared.deps.uow.commit(tx).await {
                Ok(_) => returned += 1,
                Err(err) => {
                    tracing::warn!(%err, effect = %effect.id, "a claimed dispatch effect was not returned");
                }
            }
        }
        if returned == 0 || page < usize::from(PAGE) {
            return Ok(());
        }
    }
}
