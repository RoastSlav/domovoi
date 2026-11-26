//! Settling blocks from the agent's journal when a host's agent authenticates.
//!
//! For every block of the host in `Applying`, `Applied`, `Confirming` or `RollingBack`, the hub
//! asks the journal about the executions of its phases and settles the block with the answer.
//! An `Applied` block past its 30 second kernel timeout without a confirmation is `Expired`. The
//! hub never retries a phase after dispatch, and whether an element really exists is settled
//! only by an owner-initiated refresh.

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::containment::{Token, parse_message};
use domovoi_core::ports::{ActionAnswer, BlockPhase, BlockRow, HostReconciler, PhaseUpdate};
use jiff::{SignedDuration, Timestamp};

use super::driver::ContainmentDriver;
use super::run::commit_phase;
use super::{ContainDeps, STEP_ONE_SECS, audit};
use crate::operations::reconcile_text::{ReconcileOutcome, from_answer};

#[async_trait]
impl HostReconciler for ContainmentDriver {
    async fn reconcile_host(&self, host: HostId) {
        let blocks = match self.deps().blocks.reconcilable_blocks(host).await {
            Ok(blocks) => blocks,
            Err(err) => {
                tracing::warn!(%err, %host, "could not read the containment blocks to reconcile");
                return;
            }
        };
        for block in blocks {
            reconcile_block(self.deps(), &block).await;
        }
    }
}

/// What a journal answer settles a phase to.
enum Settle {
    /// The phase finished well.
    Ok,
    /// The phase finished with this refusal or failure message.
    Failed(String),
    /// The phase never reached the journal.
    NeverRan,
    /// Nothing can establish the result.
    Unknown,
    /// Still running: ask again later.
    Pending,
}

fn settle_of(answer: ActionAnswer) -> Settle {
    match from_answer(answer) {
        None => Settle::Pending,
        Some(ReconcileOutcome::Finished {
            succeeded: true, ..
        }) => Settle::Ok,
        Some(ReconcileOutcome::Finished { detail, .. }) => {
            Settle::Failed(detail.unwrap_or_default())
        }
        Some(ReconcileOutcome::NeverRan) => Settle::NeverRan,
        Some(ReconcileOutcome::Unknown(_)) => Settle::Unknown,
    }
}

fn window_over(block: &BlockRow, now: Timestamp) -> bool {
    let started = block.applied_at.unwrap_or(block.created_at);
    started
        .checked_add(SignedDuration::from_secs(STEP_ONE_SECS))
        .is_ok_and(|end| now >= end)
}

async fn move_block(deps: &ContainDeps, block: &BlockRow, update: PhaseUpdate) {
    let kind = audit::kind_of(update.phase);
    if let Err(err) = commit_phase(deps, block, update, kind, audit::system()).await {
        tracing::debug!(%err, block = %block.block_id, "a containment reconcile was dropped");
    }
}

/// The phase a failed phase message settles to.
fn failed_phase(message: &str, window_over: bool) -> (BlockPhase, Option<String>) {
    let token = parse_message(message).map(|(token, _)| token);
    let text = message.split(": ").next().map(str::to_owned);
    match token {
        Some(Token::ContainmentLost) => (BlockPhase::Lost, text),
        Some(Token::Expired) => (BlockPhase::Expired, text),
        Some(Token::RolledBack | Token::AbsentCovered) => (BlockPhase::RolledBack, text),
        _ if window_over => (BlockPhase::Expired, text),
        _ => (BlockPhase::Failed, text),
    }
}

async fn reconcile_block(deps: &ContainDeps, block: &BlockRow) {
    let host = block.host_id;
    let now = deps.clock.now();
    // An applied block past the kernel's timeout has gone, whatever the journal says.
    if block.phase == BlockPhase::Applied {
        if window_over(block, now) {
            move_block(deps, block, PhaseUpdate::to(BlockPhase::Expired, now)).await;
        }
        return;
    }
    // No live session: the next authenticated hello asks again.
    let Some(epoch) = deps.caps.journal.epoch_of(host) else {
        return;
    };
    let wanted = match block.phase {
        BlockPhase::Applying => Some(block.block_id),
        BlockPhase::Confirming => block.confirm_execution,
        BlockPhase::RollingBack => block.rollback_execution,
        _ => None,
    };
    let Some(wanted) = wanted else {
        return;
    };
    let deadline = now.checked_add(deps.result_grace).unwrap_or(now);
    let answers = match deps
        .caps
        .journal
        .status_batch(host, vec![wanted.as_action()], epoch, deadline)
        .await
    {
        Ok(answers) => answers,
        Err(err) => {
            tracing::warn!(%err, %host, block = %block.block_id, "could not ask the agent about a containment phase");
            return;
        }
    };
    if answers.epoch != epoch {
        move_block(
            deps,
            block,
            PhaseUpdate::to(BlockPhase::OutcomeUnknown, now),
        )
        .await;
        return;
    }
    let mut own = answers
        .answers
        .into_iter()
        .filter(|entry| entry.execution == wanted.as_action())
        .map(|entry| entry.answer);
    let Some(first) = own.next() else {
        return;
    };
    // Two entries that disagree prove nothing; ask again later.
    if !own.all(|next| next == first) {
        return;
    }
    let over = window_over(block, now);
    let update = match (block.phase, settle_of(first)) {
        (_, Settle::Pending) => return,
        (_, Settle::Unknown) => PhaseUpdate::to(BlockPhase::OutcomeUnknown, now),
        (BlockPhase::Applying, Settle::Ok) => {
            let mut update = PhaseUpdate::to(BlockPhase::Applied, now);
            update.applied_at = Some(block.created_at);
            update
        }
        (BlockPhase::Applying, Settle::NeverRan) => PhaseUpdate::to(BlockPhase::NotRun, now),
        (BlockPhase::Applying, Settle::Failed(message)) => {
            let mut update = PhaseUpdate::to(BlockPhase::Failed, now);
            update.failure_token = message.split(": ").next().map(str::to_owned);
            update.failure_message = Some(message);
            update
        }
        (BlockPhase::Confirming, Settle::Ok) => {
            let mut update = PhaseUpdate::to(BlockPhase::Confirmed, now);
            update.deadline = block
                .updated_at
                .checked_add(SignedDuration::from_secs(i64::from(block.ttl_s)))
                .ok();
            update
        }
        // A confirmation that never ran leaves the block on its kernel timer.
        (BlockPhase::Confirming, Settle::NeverRan) => {
            let phase = if over {
                BlockPhase::Expired
            } else {
                BlockPhase::Applied
            };
            PhaseUpdate::to(phase, now)
        }
        (BlockPhase::Confirming | BlockPhase::RollingBack, Settle::Failed(message)) => {
            let (phase, token) = failed_phase(&message, over);
            let mut update = PhaseUpdate::to(phase, now);
            update.failure_token = token;
            update.failure_message = Some(message);
            update
        }
        (BlockPhase::RollingBack, Settle::Ok) => PhaseUpdate::to(BlockPhase::RolledBack, now),
        // A rollback that never ran leaves the block where it was.
        (BlockPhase::RollingBack, Settle::NeverRan) => {
            let phase = if block.deadline.is_some() {
                BlockPhase::Confirmed
            } else {
                BlockPhase::Applied
            };
            PhaseUpdate::to(phase, now)
        }
        _ => return,
    };
    if update.phase == BlockPhase::OutcomeUnknown {
        tracing::warn!(block = %block.block_id, "a containment block's outcome is unknown");
    }
    move_block(deps, block, update).await;
}
