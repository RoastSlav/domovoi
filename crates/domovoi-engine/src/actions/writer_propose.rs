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

/// Replaces the chat approval token hash of a still-proposed action. The previous token
/// stops working, and the action's `updated_at` does not change. A decided action is
/// refused as changed meanwhile, so a rotation never reopens a decision.
pub(super) async fn rotate_chat_token(
    shared: &Shared,
    id: ActionId,
    token_hash: [u8; 32],
) -> Decision {
    let action = load(shared, id).await?;
    if action.status() != &ActionStatus::Proposed {
        return Err(ApprovalError::Refused(ActionError::ChangedMeanwhile));
    }
    let now = shared.deps.clock.now();
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
