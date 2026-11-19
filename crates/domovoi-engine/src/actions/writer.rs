//! One writer loop applies every action decision (ADR 0019): approve, deny, the expiry
//! sweep, and every step of a run (dispatch, rejection, the answer for a run, the
//! re-check) arrive as commands on a bounded channel and are applied one at a time, so
//! exactly one decision can win an action and a decision read over a stale state is
//! refused instead of overwriting one already made. The action service runs this loop
//! joined with the executor, so whoever waits for the service also waits for the
//! decisions still queued. Expiry before dispatch is settled here too, as `NotRun`:
//! the approval window closing is positive proof the action never started.

use std::sync::Arc;

use domovoi_core::ports::{
    ActionGuard, AuditDraft, ClaimToken, CommitError, Observed, Transaction,
};
use domovoi_core::{
    ActionChange, ActionError, ActionId, ActionRequest, ActionStatus, AgentSessionId,
    ApprovalChannel, ApprovalProvenance, Authorization, EffectId, HumanApproval, PlanBinding,
    PlanHash, PlanTarget,
};
use jiff::Timestamp;

use super::approvals::{load, refused};
use super::audit::{self, RunRecord};
use super::expire;
use super::outcome::{complete, finish_effect, verify};
use super::reconcile::ReconcileOutcome;
use super::reject;
use super::{ApprovalError, Publish, Shared};
use crate::chat_token::chat_token_ok;
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
        authorization: Authorization,
        chat_token: Option<String>,
        binding: PlanBinding,
        provenance: ApprovalProvenance,
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
        /// The v2 target snapshot the plan was calculated from; absent for v1.
        target: Option<PlanTarget>,
        /// The authenticated session the plan was calculated from.
        session: AgentSessionId,
        /// That session's journal epoch, recorded when the dispatch commits.
        epoch: domovoi_core::JournalEpoch,
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
        /// The wait before the verification re-check, when one is due.
        verify_after: Option<SignedDuration>,
        reply: oneshot::Sender<Decision>,
    },
    Verified {
        id: ActionId,
        passed: bool,
        summary: String,
        /// The wake effect this answer belongs to, which is completed in the same commit.
        effect: Option<(EffectId, ClaimToken)>,
        reply: oneshot::Sender<Decision>,
    },
    FinishEffect {
        effect: EffectId,
        claim: ClaimToken,
        reply: oneshot::Sender<Result<(), ApprovalError>>,
    },
    /// Settle a dispatched run from what the agent's journal answered.
    Reconcile {
        id: ActionId,
        outcome: ReconcileOutcome,
        /// The machine-readable reason kept in the journal entry.
        wire_reason: &'static str,
        reply: oneshot::Sender<Decision>,
    },
    /// Resolve an approved action whose window closed before it was dispatched.
    ExpireApproved {
        id: ActionId,
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

/// Hands outcomes the writer queue could not take to the writer as it frees up.
///
/// One worker for the whole service, so a large batch of settled runs cannot grow a task per
/// row. It ends when the writer closes, which drops whatever is still waiting: those rows
/// stay `Dispatched` and are reconciled again later.
pub(crate) async fn feed_deferred(
    shared: Arc<Shared>,
    mut incoming: mpsc::UnboundedReceiver<WriteCommand>,
) {
    let shutdown = shared.deps.shutdown.clone();
    loop {
        let command = tokio::select! {
            () = shutdown.cancelled() => return,
            command = incoming.recv() => match command {
                Some(command) => command,
                None => return,
            },
        };
        if shared.writer.tx.send(command).await.is_err() {
            return;
        }
    }
}

/// Applies one decision and answers its caller, whatever the outcome was.
async fn process(shared: &Shared, command: WriteCommand) {
    match command {
        WriteCommand::Approve {
            id,
            authorization,
            chat_token,
            binding,
            provenance,
            reply,
        } => {
            let result = approve(
                shared,
                id,
                authorization,
                chat_token.as_deref(),
                binding,
                provenance,
            )
            .await;
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
            let result = expire::sweep(shared).await;
            if let Err(err) = &result {
                refused(err, "the expiry sweep");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Dispatch {
            id,
            plan,
            target,
            session,
            epoch,
            reply,
        } => {
            let result = super::dispatch::dispatch(shared, id, plan, target, session, epoch).await;
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
            let result = reject::reject(shared, id, reason, blocked).await;
            if let Err(err) = &result {
                refused(err, "a rejection");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Complete {
            id,
            result: outcome,
            record,
            verify_after,
            reply,
        } => {
            let result = complete(shared, id, outcome, record, verify_after).await;
            if let Err(err) = &result {
                refused(err, "the answer for a run");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Verified {
            id,
            passed,
            summary,
            effect,
            reply,
        } => {
            let result = verify(shared, id, passed, summary, effect).await;
            if let Err(err) = &result {
                refused(err, "a re-check");
            }
            let _ = reply.send(result);
        }
        WriteCommand::ExpireApproved { id, reply } => {
            let result = expire::approved(shared, id).await;
            if let Err(err) = &result {
                refused(err, "an expiry before dispatch");
            }
            let _ = reply.send(result);
        }
        WriteCommand::Reconcile {
            id,
            outcome,
            wire_reason,
            reply,
        } => {
            let result = super::reconcile::write(shared, id, outcome, wire_reason).await;
            if let Err(err) = &result {
                refused(err, "the reconciliation of a run");
            }
            let _ = reply.send(result);
        }
        WriteCommand::FinishEffect {
            effect,
            claim,
            reply,
        } => {
            let result = finish_effect(shared, effect, claim).await;
            if let Err(err) = &result {
                refused(err, "a wake effect");
            }
            let _ = reply.send(result);
        }
    }
}

/// Approves `id`; the only path that reaches `Approved`. A chat approval must
/// carry the action's own token, checked here so the answer cannot race with
/// another decision that is already queued. The binding must name the plan the
/// approver was shown; a mismatch changes nothing.
async fn approve(
    shared: &Shared,
    id: ActionId,
    authorization: Authorization,
    chat_token: Option<&str>,
    binding: PlanBinding,
    provenance: ApprovalProvenance,
) -> Result<ActionRequest, ApprovalError> {
    let mut action = load(shared, id).await?;
    // M1 has no policy permits: refuse the branch before it can touch anything.
    let Some(approval) = authorization.human_approval().cloned() else {
        return Err(ApprovalError::NotYetSupported);
    };
    if matches!(approval.channel(), ApprovalChannel::Chat { .. })
        && !chat_token_ok(chat_token, action.chat_token_hash())
    {
        return Err(ApprovalError::BadToken);
    }
    if !binding.matches(&action.plan_hash_for_approval()) {
        return Err(ApprovalError::PlanChanged);
    }
    let now = shared.deps.clock.now();
    let entry_approval = approval.clone();
    let ActionChange { from, .. } = action
        .approve(authorization, provenance, &binding, now)
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

/// The guarded write of one transition out of `from`, with its audit and journal entries.
pub(crate) async fn transition(
    shared: &Shared,
    action: &ActionRequest,
    from: &ActionStatus,
    audit: AuditDraft,
    now: Timestamp,
) -> Result<(), CommitError> {
    transition_with(shared, action, from, audit, now, |tx| tx).await
}

/// [`transition`] that also records `extra` in the journal entry, for a settled outcome
/// whose machine-readable reason the journal has to keep.
pub(crate) async fn transition_recording(
    shared: &Shared,
    action: &ActionRequest,
    from: &ActionStatus,
    audit: AuditDraft,
    now: Timestamp,
    extra: (&str, &str),
) -> Result<(), CommitError> {
    shared
        .commit_action(
            action,
            ActionGuard::Status(from.name().to_owned()),
            audit,
            now,
            Some(extra),
            Publish::ActionChanged,
        )
        .await
}

/// [`transition`] with a hook that adds effects to the same commit.
pub(crate) async fn transition_with<F>(
    shared: &Shared,
    action: &ActionRequest,
    from: &ActionStatus,
    audit: AuditDraft,
    now: Timestamp,
    with: F,
) -> Result<(), CommitError>
where
    F: FnOnce(Transaction) -> Transaction,
{
    shared
        .commit_action_with(
            action,
            ActionGuard::Status(from.name().to_owned()),
            audit,
            now,
            Publish::ActionChanged,
            with,
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
        CommitError::StaleEffect { .. } => ApprovalError::StaleEffect,
    }
}
