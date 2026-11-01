//! Approve, deny and expire. Approval is the only way into `Approved`.

use domovoi_core::ports::{ActionFilter, Page};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, ApprovalChannel, HumanApproval};

use super::{ApprovalError, Shared, audit};
use crate::events::EventKind;
use crate::triage::token_hash;

fn store(err: impl std::fmt::Display) -> ApprovalError {
    ApprovalError::Store(err.to_string())
}

/// Whether two digests are equal, in time independent of where they differ.
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn load(shared: &Shared, id: ActionId) -> Result<ActionRequest, ApprovalError> {
    shared
        .deps
        .actions
        .get(id)
        .await
        .map_err(store)?
        .ok_or(ApprovalError::NotFound(id))
}

pub(crate) async fn approve(
    shared: &Shared,
    id: ActionId,
    approval: HumanApproval,
    chat_token: Option<&str>,
) -> Result<ActionRequest, ApprovalError> {
    let mut action = load(shared, id).await?;
    if matches!(approval.channel(), ApprovalChannel::Chat { .. }) {
        let matches = match (chat_token, action.chat_token_hash()) {
            (Some(token), Some(stored)) => same(&token_hash(token), stored),
            _ => false,
        };
        if !matches {
            return Err(ApprovalError::BadToken);
        }
    }
    let now = shared.deps.clock.now();
    let entry_approval = approval.clone();
    action
        .approve(approval, now)
        .map_err(ApprovalError::Refused)?;
    action.clear_chat_token_hash();
    shared
        .record(&action, audit::approved(&action, &entry_approval, now))
        .await
        .map_err(ApprovalError::Store)?;
    shared
        .publish(&action, EventKind::ActionApproved { action: id })
        .await;
    Ok(action)
}

pub(crate) async fn deny(
    shared: &Shared,
    id: ActionId,
    approval: HumanApproval,
) -> Result<ActionRequest, ApprovalError> {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    action.deny(now).map_err(ApprovalError::Refused)?;
    action.clear_chat_token_hash();
    shared
        .record(&action, audit::denied(&action, &approval, now))
        .await
        .map_err(ApprovalError::Store)?;
    Ok(action)
}

pub(crate) async fn expire_due(shared: &Shared) -> Result<usize, ApprovalError> {
    let now = shared.deps.clock.now();
    let mut page = Page::first(Page::MAX_LIMIT);
    let mut due = Vec::new();
    loop {
        let got = shared
            .deps
            .actions
            .list(&ActionFilter::default(), &page)
            .await
            .map_err(store)?;
        due.extend(
            got.items
                .into_iter()
                .filter(|a| a.status() == &ActionStatus::Proposed && a.expires_at() <= now),
        );
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => break,
        }
    }
    let mut expired = 0;
    for mut action in due {
        if action.expire(now).is_err() {
            continue;
        }
        action.clear_chat_token_hash();
        shared
            .record(&action, audit::expired(&action, now))
            .await
            .map_err(ApprovalError::Store)?;
        expired += 1;
    }
    Ok(expired)
}
