//! One writer loop applies every action decision (ADR 0019): approve, deny, the expiry
//! sweep, and every step of a run (dispatch, rejection, the answer for a run, the
//! re-check) arrive as commands on a bounded channel and are applied one at a time, so
//! exactly one decision can win an action and a decision read over a stale state is
//! refused instead of overwriting one already made. The action service runs this loop
//! joined with the executor, so whoever waits for the service also waits for the
//! decisions still queued. It starts by failing what a previous hub session left
//! running, so a hub that restarted does not leave actions waiting forever (ADR 0014).

use std::sync::Arc;

use domovoi_core::ports::{ActionFilter, ActionGuard, AuditDraft, CommitError, Observed, Page};
use domovoi_core::{
    ActionChange, ActionError, ActionId, ActionRequest, ActionStatus, ApprovalChannel,
    HumanApproval, PlanHash,
};
use jiff::Timestamp;

use super::approvals::{chat_token_ok, load, store};
use super::audit::{self, RunRecord};
use super::recover;
use super::{ApprovalError, Publish, Shared};
use crate::events::EventKind;

use jiff::SignedDuration;
use tokio::sync::{mpsc, oneshot};

/// Commands waiting to be applied queue up at most this deep.
const WRITE_QUEUE: usize = 64;

/// What a command answers to the caller that asked for it: the action as the writer
/// left it, or why the writer refused.
pub(crate) type Decision = Result<ActionRequest, ApprovalError>;

/// A decision for the writer task; `reply` answers it back to the caller.
pub(crate) enum WriteCommand {
    Approve {
        id: ActionId,
        approval: HumanApproval,
        chat_token: Option<String>,
        reply: oneshot::Sender<Decision>,
    },
    Deny {
        id: ActionId,
        approval: HumanApproval,
        reply: oneshot::Sender<Decision>,
    },
    Propose {
        action: Box<ActionRequest>,
        reply: oneshot::Sender<Decision>,
    },
    RotateChatToken {
        id: ActionId,
        token_hash: [u8; 32],
        reply: oneshot::Sender<Decision>,
    },
    ExpireDue {
        reply: oneshot::Sender<Result<usize, ApprovalError>>,
    },
    Dispatch {
        id: ActionId,
        plan: PlanHash,
        reply: oneshot::Sender<Decision>,
    },
    Reject {
        id: ActionId,
        reason: String,
        blocked: bool,
        reply: oneshot::Sender<Decision>,
    },
    Complete {
        id: ActionId,
        result: Result<(), String>,
        record: Option<RunRecord>,
        reply: oneshot::Sender<Decision>,
    },
    Verified {
        id: ActionId,
        passed: bool,
        summary: String,
        reply: oneshot::Sender<Decision>,
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
    // A hub that restarted lost whatever it was running: an answer the agent sent
    // went to the session that is gone, so those actions are failed here (ADR 0014)
    // before any new decision can read them.
    let abandoned = match recover::fail_abandoned(&shared).await {
        Ok(count) => count,
        Err(err) => {
            tracing::error!(
                %err,
                "the store would not say what the previous session left running; those actions wait for the next hub start",
            );
            0
        }
    };
    if abandoned > 0 {
        tracing::info!(
            count = abandoned,
            "actions left running by the hub session that just stopped were failed",
        );
    }
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
        WriteCommand::Propose { action, reply } => {
            let result = super::writer_propose::propose(shared, *action).await;
            if let Err(err) = &result {
                refused(err, "a proposal");
            }
            let _ = reply.send(result);
        }
        WriteCommand::RotateChatToken {
            id,
            token_hash,
            reply,
        } => {
            let result = super::writer_propose::rotate_chat_token(shared, id, token_hash).await;
            if let Err(err) = &result {
                refused(err, "a chat token rotation");
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
        WriteCommand::Dispatch { id, plan, reply } => {
            let result = dispatch(shared, id, plan).await;
            if let Err(err) = &result {
                refused(err, "a dispatch");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Reject {
            id,
            reason,
            blocked,
            reply,
        } => {
            let result = reject(shared, id, reason, blocked).await;
            if let Err(err) = &result {
                refused(err, "a rejection");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Complete {
            id,
            result: outcome,
            record,
            reply,
        } => {
            let result = complete(shared, id, outcome, record).await;
            if let Err(err) = &result {
                refused(err, "the answer for a run");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Verified {
            id,
            passed,
            summary,
            reply,
        } => {
            let result = verify(shared, id, passed, summary).await;
            if let Err(err) = &result {
                refused(err, "a re-check");
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
    transition(
        shared,
        &action,
        &from,
        audit::approved(&action, &entry_approval, now),
        now,
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
    transition(
        shared,
        &action,
        &from,
        audit::denied(&action, &approval, now),
        now,
    )
    .await
    .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// Sends `id` to the agent: `Approved → Dispatched` when the plan the agent has now
/// still matches the approved one, `Approved → Rejected` when it does not (INV-4).
/// The audit line says which of the two happened, and the caller only runs the skill
/// when the action it gets back is `Dispatched`.
async fn dispatch(shared: &Shared, id: ActionId, plan: PlanHash) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action.dispatch(plan, now).map_err(ApprovalError::Refused)?;
    let entry = match action.status().clone() {
        ActionStatus::Rejected { reason } => audit::rejected(&action, &reason, now),
        _ => audit::dispatched(&action, now),
    };
    transition(shared, &action, &from, entry, now)
        .await
        .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// Stops `id` before it runs, for a reason the executor checked at dispatch: a loop
/// guard, a skill that is gone or changed risk, arguments no longer valid. `blocked`
/// is the loop guard, which also publishes `ActionBlocked` so the person sees why.
async fn reject(shared: &Shared, id: ActionId, reason: String, blocked: bool) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action
        .reject(reason.clone(), now)
        .map_err(ApprovalError::Refused)?;
    let entry = if blocked {
        audit::blocked(&action, &reason, now)
    } else {
        audit::rejected(&action, &reason, now)
    };
    transition(shared, &action, &from, entry, now)
        .await
        .map_err(|err| stale(err, id))?;
    if blocked {
        shared
            .publish(&action, EventKind::ActionBlocked { action: id, reason })
            .await;
    }
    Ok(action)
}

/// The agent's answer for a run: `Dispatched → Succeeded` or `Dispatched → Failed`.
/// `record` is what the run produced, `None` when the skill never ran.
async fn complete(
    shared: &Shared,
    id: ActionId,
    outcome: Result<(), String>,
    record: Option<RunRecord>,
) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action
        .complete(outcome, now)
        .map_err(ApprovalError::Refused)?;
    let entry = match action.status().clone() {
        ActionStatus::Failed { reason } => {
            audit::finished(&action, record.as_ref(), Some(&reason), now)
        }
        _ => audit::finished(&action, record.as_ref(), None, now),
    };
    transition(shared, &action, &from, entry, now)
        .await
        .map_err(|err| stale(err, id))?;
    Ok(action)
}

/// The re-check of the originating check after a successful run: `Succeeded → Verified`
/// or `Succeeded → VerifyFailed`. `summary` is what the re-check said, kept in the audit
/// line so the person can see why the incident is fixed.
async fn verify(shared: &Shared, id: ActionId, passed: bool, summary: String) -> Decision {
    let mut action = load(shared, id).await?;
    let now = shared.deps.clock.now();
    let ActionChange { from, .. } = action
        .verified(passed, now)
        .map_err(ApprovalError::Refused)?;
    transition(
        shared,
        &action,
        &from,
        audit::verification(&action, passed, &summary, now),
        now,
    )
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
        match transition(shared, &action, &from, audit::expired(&action, now), now).await {
            Ok(()) => expired += 1,
            Err(CommitError::Conflict { .. }) => {
                tracing::warn!(
                    action = %action.id(),
                    "someone else decided this action meanwhile; the sweep skips it"
                );
                continue;
            }
            Err(err) => return Err(stale(err, action.id())),
        }
    }
    Ok(expired)
}

/// The guarded write of one transition out of `from`, with its audit and journal entries.
async fn transition(
    shared: &Shared,
    action: &ActionRequest,
    from: &ActionStatus,
    audit: AuditDraft,
    now: Timestamp,
) -> Result<(), CommitError> {
    shared
        .commit_action(
            action,
            ActionGuard::Status(from.name().to_owned()),
            audit,
            now,
            None,
            Publish::ActionChanged,
        )
        .await
}

/// Why a decision's guarded write was refused: someone else decided the action
/// first (a 409 at the API), it vanished, or the store failed.
pub(crate) fn stale(err: CommitError, id: ActionId) -> ApprovalError {
    match err {
        CommitError::Conflict {
            found: Observed::Missing,
            ..
        } => ApprovalError::NotFound(id),
        CommitError::Conflict { .. } => ApprovalError::Refused(ActionError::ChangedMeanwhile),
        CommitError::Constraint(message) | CommitError::Backend(message) => {
            ApprovalError::Store(message)
        }
        CommitError::StaleEffect { .. } => ApprovalError::Store("unexpected stale effect".into()),
    }
}
