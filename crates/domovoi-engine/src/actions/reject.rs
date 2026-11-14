//! Stopping an approved action before it runs, on the writer's side.
//!
//! A rejection arrives as a command from the executor: a loop guard, a skill that is gone
//! or changed risk, arguments no longer valid, a host whose agent never came back, or an
//! approval whose incident or window no longer stands. It lands here so the transition,
//! its audit line and its journal entry are one commit, like every other decision.

use domovoi_core::ports::AuditDraft;
use domovoi_core::{ActionChange, ActionId, ActionRequest};
use jiff::Timestamp;

use super::Shared;
use super::approvals::load;
use super::audit;
use super::writer::{Decision, stale, transition};
use crate::events::EventKind;

/// Stops `id` before it runs. `blocked` is the loop guard, which also publishes
/// `ActionBlocked` so the person sees why nothing ran.
pub(crate) async fn reject(
    shared: &Shared,
    id: ActionId,
    reason: String,
    blocked: bool,
) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    stop(shared, id, &mut action, reason, blocked, now).await
}

/// Moves a loaded `Approved` action to `Rejected` with `reason`, audits it, and publishes
/// the block when the loop guard is what stopped it.
pub(crate) async fn stop(
    shared: &Shared,
    id: ActionId,
    action: &mut ActionRequest,
    reason: String,
    blocked: bool,
    now: Timestamp,
) -> Decision {
    let ActionChange { from, .. } = action
        .reject(reason.clone(), now)
        .map_err(super::ApprovalError::Refused)?;
    let entry: AuditDraft = if blocked {
        audit::blocked(action, &reason, now)
    } else {
        audit::rejected(action, &reason, now)
    };
    transition(shared, action, &from, entry, now)
        .await
        .map_err(|err| stale(err, id))?;
    if blocked {
        shared
            .publish(action, EventKind::ActionBlocked { action: id, reason })
            .await;
    }
    Ok(action.clone())
}
