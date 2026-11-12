//! One writer loop applies every action decision (ADR 0019): approve, deny and
//! the expiry sweep arrive as commands on a bounded channel and are applied one
//! at a time, so exactly one decision can win an action and a decision read over
//! a stale state is refused instead of overwriting one already made. The action
//! service runs this loop joined with the executor, so whoever waits for the
//! service also waits for this queue to drain.

use std::sync::Arc;

use domovoi_core::ports::{ActionFilter, Page, StoreError};
use domovoi_core::{
    ActionChange, ActionError, ActionId, ActionRequest, ApprovalChannel, HumanApproval,
};

use super::approvals::{chat_token_ok, load, store};
use super::audit;
use super::{ApprovalError, Shared};
use crate::events::EventKind;

use jiff::SignedDuration;
use tokio::sync::{mpsc, oneshot};

/// Commands waiting to be applied queue up at most this deep.
const WRITE_QUEUE: usize = 64;

/// A decision for the writer task; `reply` answers it back to the caller.
pub(crate) enum WriteCommand {
    Approve {
        id: ActionId,
        approval: HumanApproval,
        chat_token: Option<String>,
        reply: oneshot::Sender<Result<ActionRequest, ApprovalError>>,
    },
    Deny {
        id: ActionId,
        approval: HumanApproval,
        reply: oneshot::Sender<Result<ActionRequest, ApprovalError>>,
    },
    ExpireDue {
        reply: oneshot::Sender<Result<usize, ApprovalError>>,
    },
}

/// The queue into the writer loop, held by every [`ActionHandle`].
#[derive(Clone)]
pub(crate) struct WriterHandle {
    pub(crate) tx: mpsc::Sender<WriteCommand>,
}

/// Opens a queue deep enough for the decisions one service holds and returns its
/// reading end, which the action service runs joined with the executor loop, so
/// waiting for the service also waits for the decisions still queued. The
/// writing ends stay with the action handles.
#[must_use]
pub(crate) fn queue() -> (WriterHandle, mpsc::Receiver<WriteCommand>) {
    let (tx, rx) = mpsc::channel(WRITE_QUEUE);
    (WriterHandle { tx: tx.clone() }, rx)
}

/// Applies decisions one at a time until shutdown closes the queue or every
/// handle is dropped, then applies what is still queued, so a hub that waits
/// for the action service also waits for those writes to reach the store.
#[must_use]
pub(crate) async fn run(shared: Arc<Shared>, mut rx: mpsc::Receiver<WriteCommand>) {
    let shutdown = shared.deps.shutdown.clone();
    loop {
        let command = tokio::select! {
            () = shutdown.cancelled() => break,
            command = rx.recv() => match command {
                Some(command) => command,
                None => break,
            },
        };
        process(&shared, command).await;
    }
    while let Ok(command) = rx.try_recv() {
        process(&shared, command).await;
    }
}

/// Applies one decision and answers its caller, whatever the outcome was.
async fn process(shared: &Shared, command: WriteCommand) {
    match command {
        WriteCommand::Approve {
            id,
            approval,
            chat_token,
            reply,
        } => {
            let result = approve(shared, id, approval, chat_token.as_deref()).await;
            if let Err(err) = &result {
                refused(err, "an approval");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Deny {
            id,
            approval,
            reply,
        } => {
            let result = deny(shared, id, approval).await;
            if let Err(err) = &result {
                refused(err, "a denial");
            }
            let _ = reply.send(result);
        }
        WriteCommand::ExpireDue { reply } => {
            let result = expire_due(shared).await;
            if let Err(err) = &result {
                refused(err, "the expiry sweep");
            }
            let _ = reply.send(result);
        }
    }
}

/// Logs one refusal at the level it deserves. The caller gets every one of these
/// back, so a missing action, a used chat token, an action that already has an
/// outcome and a shutdown are debug: the person sees why nothing happened. A
/// store failure warns because the answer the caller gets says only that the
/// store failed, and the reason is worth a line in the log.
fn refused(err: &ApprovalError, what: &str) {
    match err {
        ApprovalError::Store(why) => tracing::warn!(%why, "the store refused {what}"),
        ApprovalError::NotFound(id) => {
            tracing::debug!(%id, "{what} asked for an action that is not there")
        }
        ApprovalError::BadToken => tracing::debug!("{what} had no working chat token"),
        ApprovalError::Refused(reason) => {
            tracing::debug!(%reason, "the action could not take {what}")
        }
        ApprovalError::Unavailable => tracing::debug!("{what} did not reach the writer"),
    }
}

/// Approves `id`; the only path that reaches `Approved`. A chat approval must
/// carry the action's own token, checked here so the answer cannot race with
/// another decision that is already queued.
async fn approve(
    shared: &Shared,
    id: ActionId,
    approval: HumanApproval,
    chat_token: Option<&str>,
) -> Result<ActionRequest, ApprovalError> {
    let mut action = load(shared, id).await?;
    if matches!(approval.channel(), ApprovalChannel::Chat { .. })
        && !chat_token_ok(chat_token, &action)
    {
        return Err(ApprovalError::BadToken);
    }
    let now = shared.deps.clock.now();
    let entry_approval = approval.clone();
    let ActionChange { from, .. } = action
        .approve(approval, now)
        .map_err(ApprovalError::Refused)?;
    action.clear_chat_token_hash();
    shared
        .record_from(
            &action,
            from.name(),
            audit::approved(&action, &entry_approval, now),
        )
        .await
        .map_err(|err| stale(err, id))?;
    shared
        .publish(&action, EventKind::ActionApproved { action: id })
        .await;
    Ok(action)
}

/// Denies `id` on behalf of the human in `approval`.
async fn deny(
    shared: &Shared,
    id: ActionId,
    approval: HumanApproval,
) -> Result<ActionRequest, ApprovalError> {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action.deny(now).map_err(ApprovalError::Refused)?;
    action.clear_chat_token_hash();
    shared
        .record_from(&action, from.name(), audit::denied(&action, &approval, now))
        .await
        .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// Expires every still-proposed action whose window has closed and counts them.
/// The store query does the picking, so an action decided meanwhile never shows
/// up here; if one was nevertheless read open, the guarded write refuses it and
/// the sweep skips it.
async fn expire_due(shared: &Shared) -> Result<usize, ApprovalError> {
    let now = shared.deps.clock.now();
    let deadline = now
        .checked_add(SignedDuration::from_millis(1))
        .unwrap_or(now);
    let filter = ActionFilter {
        status: Some("proposed".to_owned()),
        expires_before: Some(deadline),
        ..ActionFilter::default()
    };
    let mut page = Page::first(Page::MAX_LIMIT);
    let mut due = Vec::new();
    loop {
        let got = shared
            .deps
            .actions
            .list(&filter, &page)
            .await
            .map_err(store)?;
        due.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => break,
        }
    }
    let mut expired = 0;
    for mut action in due {
        let from = match action.expire(now) {
            Ok(ActionChange { from, .. }) => from,
            Err(_) => continue,
        };
        action.clear_chat_token_hash();
        match shared
            .record_from(&action, from.name(), audit::expired(&action, now))
            .await
        {
            Ok(()) => expired += 1,
            Err(StoreError::Backend(what)) => return Err(ApprovalError::Store(what)),
            Err(err) => {
                tracing::warn!(
                    %err,
                    action = %action.id(),
                    "someone else decided this action meanwhile; the sweep skips it"
                );
                continue;
            }
        }
    }
    Ok(expired)
}

/// Why a decision's guarded write was refused: someone else decided the action
/// first (a 409 at the API), it vanished, or the store failed.
fn stale(err: StoreError, id: ActionId) -> ApprovalError {
    match err {
        StoreError::Conflict(_) => ApprovalError::Refused(ActionError::ChangedMeanwhile),
        StoreError::NotFound => ApprovalError::NotFound(id),
        StoreError::Backend(what) => ApprovalError::Store(what),
    }
}
