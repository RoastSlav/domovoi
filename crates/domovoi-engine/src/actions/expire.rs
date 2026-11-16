//! Settling actions whose approval window closed before anything ran.
//!
//! A `Proposed` action nobody decided becomes `Expired`. An `Approved` action whose window
//! closed before it was dispatched becomes `NotRun`: the closed window is positive proof
//! the skill never reached the agent, so it is neither a failure nor an unknown outcome.
//! The sweep reads both by window from the store, so an action decided meanwhile never
//! shows up here and a guarded write that loses the race is skipped rather than retried.

use domovoi_core::ports::{ActionFilter, CommitError, Page, StoreError};
use domovoi_core::{ActionChange, ActionError, ActionId, ActionRequest, ActionStatus};
use jiff::{SignedDuration, Timestamp};

use super::approvals::load;
use super::audit;
use super::reconcile::{EXPIRED_REASON, EXPIRED_TOKEN};
use super::writer::{Decision, stale, transition, transition_recording};
use super::{ApprovalError, Shared};

/// Expires every proposed action and every approved action whose window has closed, and
/// counts them. Handing back `Ok(0)` means there was nothing to settle, not that the store
/// was too busy to say.
pub(crate) async fn sweep(shared: &Shared) -> Result<usize, ApprovalError> {
    let now = shared.deps.clock.now();
    let deadline = now
        .checked_add(SignedDuration::from_millis(1))
        .unwrap_or(now);
    let mut settled = 0;
    for mut action in due(shared, "proposed", deadline).await? {
        let from = match action.expire(now) {
            Ok(ActionChange { from, .. }) => from,
            Err(_) => continue,
        };
        action.clear_chat_token_hash();
        match transition(shared, &action, &from, audit::expired(&action, now), now).await {
            Ok(()) => settled += 1,
            Err(CommitError::Conflict { .. }) => {
                tracing::warn!(
                    action = %action.id(),
                    "someone else decided this action meanwhile; the sweep skips it"
                );
            }
            Err(err) => return Err(stale(err, action.id())),
        }
    }
    for mut action in due(shared, "approved", deadline).await? {
        let id = action.id();
        match approved_loaded(shared, id, &mut action, now).await {
            Ok(_) => settled += 1,
            Err(err @ ApprovalError::Store(_)) => return Err(err),
            Err(err) => tracing::warn!(
                action = %id,
                %err,
                "an approved action changed as its window closed; the sweep skips it",
            ),
        }
    }
    Ok(settled)
}

/// Every action in `status` whose approval window closes at `deadline`, paged to the end.
///
/// A page that fails fails the whole sweep: a short list would leave the rest unsettled
/// while looking like there was nothing left to do.
async fn due(
    shared: &Shared,
    status: &str,
    deadline: Timestamp,
) -> Result<Vec<ActionRequest>, ApprovalError> {
    let filter = ActionFilter {
        status: Some(status.to_owned()),
        expires_before: Some(deadline),
        ..ActionFilter::default()
    };
    let mut page = Page::first(Page::MAX_LIMIT);
    let mut found = Vec::new();
    loop {
        let got = shared
            .deps
            .actions
            .list(&filter, &page)
            .await
            .map_err(store)?;
        found.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(found),
        }
    }
}

/// Expires `id` before dispatch, for whoever read it as approved and must stop.
pub(crate) async fn approved(shared: &Shared, id: ActionId) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    approved_loaded(shared, id, &mut action, now).await
}

/// The `Approved → NotRun` transition for a loaded action.
///
/// Only an approved action is expired here: `never_ran` also settles a dispatched run,
/// which a closed window says nothing about, so the status is checked rather than taken on
/// trust.
pub(crate) async fn approved_loaded(
    shared: &Shared,
    id: ActionId,
    action: &mut ActionRequest,
    now: Timestamp,
) -> Decision {
    if action.status() != &ActionStatus::Approved {
        return Err(ApprovalError::Refused(ActionError::ChangedMeanwhile));
    }
    let ActionChange { from, .. } = action
        .never_ran(EXPIRED_REASON.to_owned(), now)
        .map_err(ApprovalError::Refused)?;
    action.clear_chat_token_hash();
    let entry = audit::not_run(action, EXPIRED_REASON, now);
    transition_recording(
        shared,
        action,
        &from,
        entry,
        now,
        ("outcome_reason", EXPIRED_TOKEN),
    )
    .await
    .map_err(|err| stale(err, id))?;
    Ok(action.clone())
}

/// The store failed, as an approval error.
fn store(err: StoreError) -> ApprovalError {
    ApprovalError::Store(err.to_string())
}
