//! Reconciling runs the hub dispatched but can no longer see the result of.
//!
//! A hub that restarts keeps every run it sent as `Dispatched`: the agent may have run it,
//! and a restart is no evidence either way. When that host's agent authenticates, this
//! pass asks its action journal what became of each run and commits exactly what the
//! journal answers: a completed result, `NotRun` for positive evidence the request never
//! arrived, or `OutcomeUnknown` when the outcome cannot be established. Uncertainty is
//! never turned into success or failure. The same module owns the sentences and audit
//! reasons for expiry before dispatch, the other positive proof that nothing started.

use std::collections::BTreeSet;

use domovoi_core::ports::{ActionAnswer, ActionFilter, Page, StoreError, UnknownReason};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, HostId};
use tokio::sync::oneshot;

use super::Shared;
use super::writer::{Decision, WriteCommand};

/// Positive evidence the agent's journal never received the request.
pub(crate) const NOT_RUN_REASON: &str = "the agent has no record of this run; nothing was started";
/// The journal recorded acceptance but not a start.
pub(crate) const NO_START_REASON: &str =
    "the agent recorded this run but not its start; it may not have run";
/// The agent restarted while the run was recorded as started.
pub(crate) const AGENT_RESTARTED_REASON: &str =
    "the agent restarted while this ran; check the host before running it again";
/// The journal lost its record, or the hub asked under a stale epoch.
pub(crate) const JOURNAL_LOST_REASON: &str =
    "the agent lost its record of this run; check the host before running it again";
/// The journal was reset since the run was dispatched.
pub(crate) const EPOCH_CHANGED_REASON: &str =
    "the agent's journal was reset since this ran; the result cannot be recovered";
/// The run was dispatched before the hub kept an epoch.
pub(crate) const EPOCH_UNKNOWN_REASON: &str =
    "this ran before the agent kept a journal; the result cannot be recovered";
/// The approval window closed before the action was sent.
pub(crate) const EXPIRED_REASON: &str = "the approval expired before the action was dispatched";
/// The token for the same, kept in the journal entry's `outcome_reason`.
pub(crate) const EXPIRED_TOKEN: &str = "expired";

/// How many times one pass asks again about runs that are still running. The rows stay
/// `Dispatched` when the bound is reached; they are never failed.
pub(crate) const RECONCILE_ROUNDS: u32 = 240;

/// The most ids one `action_status` request may carry.
const BATCH: usize = 64;

/// What one journal answer or epoch comparison settles an action to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReconcileOutcome {
    /// The run finished and the recorded result is authoritative.
    Finished {
        /// Whether that result is a success.
        succeeded: bool,
        /// The agent's failure message, when it failed.
        detail: Option<String>,
    },
    /// The outcome cannot be established.
    Unknown(UnknownReason),
    /// Positive evidence the run never reached the agent.
    NeverRan,
}

impl ReconcileOutcome {
    /// The machine-readable token kept in the journal entry's `outcome_reason`.
    pub(crate) fn token(&self) -> &'static str {
        match self {
            Self::Finished { .. } => "finished",
            Self::NeverRan => "not_run",
            Self::Unknown(UnknownReason::NoStartRecord) => "no_start_record",
            Self::Unknown(UnknownReason::AgentRestarted) => "agent_restarted",
            Self::Unknown(UnknownReason::JournalLost) => "journal_lost",
            Self::Unknown(UnknownReason::EpochChanged) => "epoch_changed",
            Self::Unknown(UnknownReason::EpochUnknown) => "epoch_unknown",
        }
    }
}

/// The sentence for a journal reason the hub decided or the agent reported.
pub(crate) const fn reason_text(reason: UnknownReason) -> &'static str {
    match reason {
        UnknownReason::NoStartRecord => NO_START_REASON,
        UnknownReason::AgentRestarted => AGENT_RESTARTED_REASON,
        UnknownReason::JournalLost => JOURNAL_LOST_REASON,
        UnknownReason::EpochChanged => EPOCH_CHANGED_REASON,
        UnknownReason::EpochUnknown => EPOCH_UNKNOWN_REASON,
    }
}

/// What `answer` settles the run to, or `None` while it is still running and must be asked
/// about again.
pub(crate) fn from_answer(answer: ActionAnswer) -> Option<ReconcileOutcome> {
    match answer {
        ActionAnswer::Finished { succeeded, detail } => {
            Some(ReconcileOutcome::Finished { succeeded, detail })
        }
        ActionAnswer::Running => None,
        ActionAnswer::NotFound => Some(ReconcileOutcome::NeverRan),
        ActionAnswer::Unknown { reason } => Some(ReconcileOutcome::Unknown(reason)),
    }
}

/// Reconciles every run dispatched to `host` against its agent's journal. A host with no
/// live session has no answer, so nothing is read or written for it.
pub(crate) async fn host(shared: &Shared, host: HostId) {
    let Some(epoch) = shared.deps.link.journal_epoch(host) else {
        return;
    };
    let pending = match pending(shared, host).await {
        Ok(pending) => pending,
        Err(err) => {
            tracing::error!(
                %err,
                %host,
                "the store would not list dispatched actions; nothing is reconciled this time",
            );
            return;
        }
    };
    let mut remaining = Vec::new();
    for action in pending {
        match action.status() {
            ActionStatus::Dispatched { agent_epoch: None } => {
                settle(
                    shared,
                    action.id(),
                    ReconcileOutcome::Unknown(UnknownReason::EpochUnknown),
                )
                .await;
            }
            ActionStatus::Dispatched {
                agent_epoch: Some(seen),
            } if *seen != epoch => {
                settle(
                    shared,
                    action.id(),
                    ReconcileOutcome::Unknown(UnknownReason::EpochChanged),
                )
                .await;
            }
            _ => remaining.push(action.id()),
        }
    }
    let mut rounds = 0;
    loop {
        let mut running = Vec::new();
        for batch in remaining.chunks(BATCH) {
            let now = shared.deps.clock.now();
            let deadline = now
                .checked_add(shared.settings.reconcile_query_timeout)
                .unwrap_or(now);
            let answers = match shared
                .deps
                .link
                .action_status(host, batch.to_vec(), epoch, deadline)
                .await
            {
                Ok(answers) => answers,
                Err(err) => {
                    tracing::warn!(
                        %err,
                        %host,
                        count = batch.len(),
                        "could not ask the agent about dispatched runs; they stay dispatched",
                    );
                    continue;
                }
            };
            if answers.epoch != epoch {
                for id in batch {
                    settle(
                        shared,
                        *id,
                        ReconcileOutcome::Unknown(UnknownReason::EpochChanged),
                    )
                    .await;
                }
                continue;
            }
            let asked: BTreeSet<ActionId> = batch.iter().copied().collect();
            let mut answered: BTreeSet<ActionId> = BTreeSet::new();
            for entry in answers.answers {
                if !asked.contains(&entry.execution) {
                    tracing::warn!(
                        %host,
                        action = %entry.execution,
                        "the agent answered about a run the hub did not ask about; ignored",
                    );
                    continue;
                }
                answered.insert(entry.execution);
                match from_answer(entry.answer) {
                    Some(outcome) => settle(shared, entry.execution, outcome).await,
                    None => running.push(entry.execution),
                }
            }
            for id in asked.difference(&answered) {
                // The protocol answers one per requested id. An agent that says nothing
                // about a run has given no evidence either way, so it is asked again.
                tracing::warn!(
                    %host,
                    action = %id,
                    "the agent did not answer about a dispatched run; it stays dispatched",
                );
                running.push(*id);
            }
        }
        if running.is_empty() {
            return;
        }
        rounds += 1;
        if rounds > RECONCILE_ROUNDS {
            tracing::warn!(
                %host,
                rounds,
                "dispatched runs are still running; reconciliation stops asking for now",
            );
            return;
        }
        if shared.deps.link.journal_epoch(host) != Some(epoch) {
            return;
        }
        let now = shared.deps.clock.now();
        let next = now
            .checked_add(shared.settings.reconcile_interval)
            .unwrap_or(now);
        shared.deps.clock.sleep_until(next).await;
        remaining = running;
    }
}

/// Every action still `Dispatched` on `host`, paged to the end. A short list is not a
/// complete list, so a failed page fails the whole pass.
async fn pending(shared: &Shared, host: HostId) -> Result<Vec<ActionRequest>, StoreError> {
    let filter = ActionFilter {
        host: Some(host),
        status: Some("dispatched".to_owned()),
        ..ActionFilter::default()
    };
    let mut found = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = shared.deps.actions.list(&filter, &page).await?;
        found.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(found),
        }
    }
}

/// Queues one settled outcome for the writer, which commits it under the `dispatched`
/// guard. A refused write is logged and dropped: the row keeps whatever it now holds.
async fn settle(shared: &Shared, id: ActionId, outcome: ReconcileOutcome) {
    let wire_reason = outcome.token();
    let (reply, answer) = oneshot::channel();
    let command = WriteCommand::Reconcile {
        id,
        outcome,
        wire_reason,
        reply,
    };
    if shared.writer.tx.send(command).await.is_err() {
        tracing::debug!(action = %id, "the action service is closing; the run stays dispatched");
        return;
    }
    match answer.await {
        Ok(Ok(_)) => {}
        Ok(Err(err)) => {
            tracing::debug!(action = %id, %err, "reconciliation's write was refused; nothing changed")
        }
        Err(_) => {
            tracing::debug!(action = %id, "the writer stopped before answering reconciliation")
        }
    }
}

/// `Dispatched → OutcomeUnknown`, `Dispatched → NotRun` or `Dispatched → Succeeded/Failed`
/// for one answer, committed by the writer.
pub(crate) async fn write(
    shared: &Shared,
    id: ActionId,
    outcome: ReconcileOutcome,
    wire_reason: &'static str,
) -> Decision {
    let mut action = super::approvals::load(shared, id).await?;
    let now = shared.deps.clock.now();
    let _from = match &outcome {
        ReconcileOutcome::Finished { succeeded, detail } => {
            let result = if *succeeded {
                Ok(())
            } else {
                Err(detail
                    .clone()
                    .unwrap_or_else(|| "the run failed".to_owned()))
            };
            action
                .complete(result, now)
                .map_err(super::ApprovalError::Refused)?
                .from
        }
        ReconcileOutcome::Unknown(reason) => {
            let text = reason_text(*reason).to_owned();
            action
                .outcome_unknown(text, now)
                .map_err(super::ApprovalError::Refused)?
                .from
        }
        ReconcileOutcome::NeverRan => {
            action
                .never_ran(NOT_RUN_REASON.to_owned(), now)
                .map_err(super::ApprovalError::Refused)?
                .from
        }
    };
    let entry = match action.status() {
        ActionStatus::Succeeded => super::audit::finished(&action, None, None, now),
        ActionStatus::Failed { reason } => super::audit::finished(&action, None, Some(reason), now),
        ActionStatus::OutcomeUnknown { reason } => {
            super::audit::outcome_unknown(&action, reason, now)
        }
        ActionStatus::NotRun { reason } => super::audit::not_run(&action, reason, now),
        _ => {
            return Err(super::ApprovalError::Refused(
                domovoi_core::ActionError::ChangedMeanwhile,
            ));
        }
    };
    shared
        .commit_action(
            &action,
            domovoi_core::ports::ActionGuard::Status("dispatched".to_owned()),
            entry,
            now,
            Some(("outcome_reason", wire_reason)),
            super::Publish::ActionChanged,
        )
        .await
        .map_err(|err| super::writer::stale(err, id))?;
    Ok(action)
}
