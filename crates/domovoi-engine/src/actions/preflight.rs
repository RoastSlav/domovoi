//! The checks an approved action goes through before it is sent: freshness, the loop guard
//! and re-validation against the agent's current catalogue.
//!
//! The executor asks these; nothing here writes action state. A stale approval is queued for
//! the writer as an expiry or a rejection, so the one commit path stays the writer's.

use domovoi_core::{ActionId, ActionRequest, SkillSummary, ValidatedArgs};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::oneshot;

use super::Shared;
use super::executor::ask;
use super::freshness;
use super::writer::WriteCommand;

/// Queues a rejection made before dispatch. `blocked` is the loop guard, which also
/// publishes `ActionBlocked` so the person knows why nothing ran.
pub(crate) async fn ask_reject(
    shared: &Shared,
    id: ActionId,
    reason: String,
    blocked: bool,
) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "a rejection",
        WriteCommand::Reject {
            id,
            reason,
            blocked,
            reply,
        },
        answer,
    )
    .await
}

/// Queues the expiry of an approved action whose window closed while it waited. The
/// action becomes `NotRun`: the window closing is proof it never reached the agent.
pub(crate) async fn ask_expire(shared: &Shared, id: ActionId) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "an expiry before dispatch",
        WriteCommand::ExpireApproved { id, reply },
        answer,
    )
    .await
}
/// Takes one loop-guard slot for this action, or says why it cannot run: the skill already
/// used every slot on this host in the last hour. The count and the claim are one lock, so
/// runs that the service started together from the store cannot all read the same count and
/// all pass; the runs a previous hub session made are already in the bookkeeping, which the
/// recovery scan put back from the audit log.
///
/// The guard counts committed dispatch attempts, not completed runs: a run the hub proved it
/// never handed over is still an attempt, and it keeps its `action.dispatched` audit row, so
/// the in-memory count and the count rebuilt from the audit log after a restart agree.
pub(crate) fn reserve_dispatch(
    shared: &Shared,
    action: &ActionRequest,
    now: Timestamp,
) -> Option<String> {
    let hour_ago = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let mut dispatches = shared
        .dispatches
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let runs = dispatches
        .entry((action.host(), action.skill().to_string()))
        .or_default();
    while runs.front().is_some_and(|t| *t <= hour_ago) {
        runs.pop_front();
    }
    let limit = shared.settings.loop_guard_per_hour;
    if runs.len() >= limit as usize {
        return Some(format!(
            "loop guard: {} already ran {limit} times on this host in the last hour",
            action.skill(),
        ));
    }
    runs.push_back(now);
    None
}

/// The skill as the agent has it now and the stored arguments validated against it, or
/// the reason the action must be rejected before dispatch (INV-4). The rejection itself
/// is queued for the writer, which audits it.
pub(crate) fn revalidate(
    shared: &Shared,
    action: &ActionRequest,
) -> Result<(SkillSummary, ValidatedArgs), String> {
    let skills = shared.deps.link.skills(action.host());
    let found = skills.into_iter().find(|s| &s.name == action.skill());
    match found {
        None => Err("the skill is no longer on the host".to_owned()),
        Some(skill) if skill.risk != action.risk() => {
            Err(format!("the skill's risk changed to {:?}", skill.risk))
        }
        Some(skill) => skill
            .validate_args(action.args())
            .map(|args| (skill, args))
            .map_err(|err| format!("the arguments are no longer valid: {err}")),
    }
}

/// Whether `action` may still be dispatched at `now`, rejecting it with the reason when
/// it may not. A store that cannot say leaves it approved: nothing is sent and the next
/// attempt, or the next hub start, asks again.
pub(crate) async fn still_current(
    shared: &Shared,
    id: ActionId,
    action: &ActionRequest,
    now: Timestamp,
) -> bool {
    match freshness::check(shared, action, now).await {
        Ok(None) => true,
        Ok(Some(old)) => {
            if old == freshness::Stale::Expired {
                let _ = ask_expire(shared, id).await;
            } else {
                let _ = ask_reject(shared, id, old.reason().to_owned(), false).await;
            }
            false
        }
        Err(err) => {
            tracing::error!(
                %err,
                action = %id,
                "could not check whether the approval is still current; it stays approved",
            );
            false
        }
    }
}
