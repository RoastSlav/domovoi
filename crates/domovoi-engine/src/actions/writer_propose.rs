//! The writer commands triage uses: storing a proposal, and replacing the chat approval
//! token of one still waiting. Both go through the writer like every other action write,
//! so the stored action, its audit entry and its journal entry land in one commit.

use domovoi_core::ports::ActionGuard;
use domovoi_core::{ActionError, ActionId, ActionRequest, ActionStatus};

use super::approvals::load;
use super::audit;
use super::writer::{Decision, stale};
use super::{ApprovalError, Publish, Shared};

/// Stores a newly proposed action with its audit and journal entries. Only a `proposed`
/// action can be stored, so a proposal can never enter the approval path.
pub(super) async fn propose(shared: &Shared, action: ActionRequest) -> Decision {
    if action.status() != &ActionStatus::Proposed {
        return Err(ApprovalError::Refused(ActionError::NotAllowed {
            status: action.status().clone(),
            operation: "propose",
        }));
    }
    let id = action.id();
    let now = shared.deps.clock.now();
    shared
        .commit_action(
            &action,
            ActionGuard::New,
            audit::proposed(&action, now),
            now,
            None,
            Publish::Nothing,
        )
        .await
        .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// Replaces the chat approval token hash of a still-proposed, unexpired action. The previous
/// token stops working, and neither `updated_at` nor `expires_at` changes. A decided action
/// is refused as changed meanwhile, so a rotation never reopens a decision, and an action
/// whose approval window has closed is refused as expired so triage can store a fresh one.
pub(super) async fn rotate_chat_token(
    shared: &Shared,
    id: ActionId,
    token_hash: [u8; 32],
) -> Decision {
    let action = load(shared, id).await?;
    match action.status() {
        ActionStatus::Proposed => {}
        // The expiry sweep already settled it: this is still an expiry, not a decision.
        ActionStatus::Expired => return Err(ApprovalError::Refused(ActionError::Expired)),
        _ => return Err(ApprovalError::Refused(ActionError::ChangedMeanwhile)),
    }
    let now = shared.deps.clock.now();
    // The window may have closed after the caller read the action but before this command
    // reached the writer. Check the writer's own clock and leave the row for the sweep.
    if action.expires_at() <= now {
        return Err(ApprovalError::Refused(ActionError::Expired));
    }
    let rotated = action.with_chat_token_hash(token_hash);
    shared
        .commit_action(
            &rotated,
            ActionGuard::Status(ActionStatus::Proposed.name().to_owned()),
            audit::token_rotated(&rotated, now),
            now,
            Some(("change", "chat_token_rotated")),
            Publish::Nothing,
        )
        .await
        .map_err(|err| stale(err, id))?;
    Ok(rotated)
}
