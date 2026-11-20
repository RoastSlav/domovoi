//! Reconciling runs the hub dispatched but can no longer see the result of.
//!
//! A hub that restarts keeps every run it sent as `Dispatched`: the agent may have run it,
//! and a restart is no evidence either way. When that host's agent authenticates, this
//! pass asks its action journal what became of each run and commits exactly what the
//! journal answers: a completed result, `NotRun` for positive evidence the request never
//! arrived, or `OutcomeUnknown` when the outcome cannot be established. Uncertainty is
//! never turned into success or failure. The same module owns the sentences and audit
//! reasons for expiry before dispatch, the other positive proof that nothing started.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_core::ports::{ActionFilter, Page, StoreError, UnknownReason};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, HostId};
use tokio::sync::{mpsc, oneshot};

use super::Shared;
use super::writer::{Decision, WriteCommand};

#[cfg(test)]
mod guard_tests;
mod queue;

pub(crate) use crate::operations::reconcile_text::{
    EXPIRED_REASON, EXPIRED_TOKEN, HANDOFF_FAILED_REASON, NOT_RUN_REASON, ReconcileOutcome,
    from_answer, reason_text,
};
pub use queue::MAX_RECONCILE_PASSES;
pub(crate) use queue::{handoff_failed, serve};

/// How many times a pass re-reads the complete pending list when a page fails, starting
/// over from the first page each time. A short scan is never treated as a complete one.
pub(crate) const RECONCILE_LISTING_ATTEMPTS: u32 = 3;

/// The pause between listing attempts, so a store that is failing is not hammered. It is
/// kept short: the retry exists for a hiccup, not for a store that is down.
pub(crate) const RECONCILE_LISTING_BACKOFF: jiff::SignedDuration =
    jiff::SignedDuration::from_millis(250);

/// How many times one pass asks again about runs that are still running, after its first
/// query: one pass makes at most [`RECONCILE_REPEATS`] + 1 queries, about two hours apart at
/// the default interval. The rows stay `Dispatched` when the bound is reached; they are
/// never failed.
pub(crate) const RECONCILE_REPEATS: u32 = 240;

/// Whether a pass that has made `repeats` repeat queries has used them all up.
pub(crate) const fn repeats_exhausted(repeats: u32) -> bool {
    repeats >= RECONCILE_REPEATS
}

/// How many rounds a pass keeps asking after the agent fails to answer its query. A lost
/// query is not an outcome, but a broken link must not keep a pass alive either.
pub(crate) const RECONCILE_QUERY_ATTEMPTS: u32 = 3;

/// The most ids one `action_status` request may carry.
const BATCH: usize = 64;

/// Reconciles every run dispatched to `host` against its agent's journal, waiting for any
/// pass already running for that host and then for one of the bounded pass slots. A host with
/// no live session has no answer, so nothing is read or written for it.
///
/// The host gate comes first: a session that reconnects while a pass still owns its host must
/// wait without holding a global slot, so the pool stays available to other hosts.
pub(crate) async fn host(shared: &Arc<Shared>, host: HostId) {
    let _pass = queue::hold_pass(shared.clone(), host).await;
    let Some(_slot) = queue::take_pass_slot(shared).await else {
        return;
    };
    pass(shared, host).await;
}

/// One reconciliation pass; the caller holds the host's pass gate.
async fn pass(shared: &Shared, host: HostId) {
    let mut writes = Vec::new();
    pass_rows(shared, host, &mut writes).await;
    // Wait for what was queued, so a caller that asked for the pass sees its effect. The
    // commands are already handed over, so a cancelled caller loses only this wait.
    for answer in writes {
        let _ = answer.await;
    }
}

/// The rows a pass settles, collecting where the writer will answer.
async fn pass_rows(shared: &Shared, host: HostId, writes: &mut Vec<oneshot::Receiver<Decision>>) {
    let Some(epoch) = shared.deps.journal.epoch_of(host) else {
        return;
    };
    // Take the host's dispatch gate before reading what is pending, so a dispatch that was
    // committed but has not reached the agent's queue is not in the snapshot. The gate is
    // released before any journal query, which may take a while.
    let Some(pending) = snapshot(shared, host).await else {
        return;
    };
    // The session may have been replaced while the rows were being read. Classifying them
    // against the epoch that is gone would make a new session's runs look foreign, so the
    // pass stops instead: the replacement's own pass will settle them.
    if shared.deps.journal.epoch_of(host) != Some(epoch) {
        tracing::debug!(
            %host,
            "the agent's journal changed while dispatched runs were being read; not reconciling",
        );
        return;
    }
    let mut remaining = Vec::new();
    for action in pending {
        match action.status() {
            ActionStatus::Dispatched { agent_epoch: None } => {
                queue(
                    shared,
                    writes,
                    action.id(),
                    ReconcileOutcome::Unknown(UnknownReason::EpochUnknown),
                );
            }
            ActionStatus::Dispatched {
                agent_epoch: Some(seen),
            } if *seen != epoch => {
                queue(
                    shared,
                    writes,
                    action.id(),
                    ReconcileOutcome::Unknown(UnknownReason::EpochChanged),
                );
            }
            _ => remaining.push(action.id()),
        }
    }
    let mut repeats = 0;
    loop {
        let mut running = Vec::new();
        for batch in remaining.chunks(BATCH) {
            // A replaced session must not be asked, and its epoch must not be used to
            // classify anything: stop before the query rather than after.
            if shared.deps.journal.epoch_of(host) != Some(epoch) {
                tracing::debug!(
                    %host,
                    "the agent's journal changed during reconciliation; stopping this pass",
                );
                return;
            }
            // A lost query is not an outcome. Try again at once, a bounded number of times,
            // so a transient failure does not silently use up the only reconciliation
            // attempt while the session stays connected.
            let mut tries = 0;
            let answers = loop {
                let now = shared.deps.clock.now();
                let deadline = now
                    .checked_add(shared.settings.reconcile_query_timeout)
                    .unwrap_or(now);
                match shared
                    .deps
                    .journal
                    .status_batch(host, batch.to_vec(), epoch, deadline)
                    .await
                {
                    Ok(answers) => break Some(answers),
                    Err(err) => {
                        tries += 1;
                        tracing::warn!(
                            %err,
                            %host,
                            count = batch.len(),
                            attempt = tries,
                            "could not ask the agent about dispatched runs; they stay dispatched",
                        );
                        if tries > RECONCILE_QUERY_ATTEMPTS {
                            tracing::warn!(
                                %host,
                                attempts = tries,
                                "the agent did not answer about dispatched runs; reconciliation stops asking",
                            );
                            break None;
                        }
                    }
                }
            };
            let Some(answers) = answers else { continue };
            if answers.epoch != epoch {
                for id in batch {
                    queue(
                        shared,
                        writes,
                        *id,
                        ReconcileOutcome::Unknown(UnknownReason::EpochChanged),
                    );
                }
                continue;
            }
            let asked: BTreeSet<ActionId> = batch.iter().copied().collect();
            let mut seen: BTreeMap<ActionId, u32> = BTreeMap::new();
            for entry in &answers.answers {
                if asked.contains(&entry.execution) {
                    *seen.entry(entry.execution).or_default() += 1;
                } else {
                    tracing::warn!(
                        %host,
                        action = %entry.execution,
                        "the agent answered about a run the hub did not ask about; ignored",
                    );
                }
            }
            // One answer per requested id is the protocol. An id answered twice is a
            // malformed response, and neither entry may settle it: choosing one would make
            // the outcome depend on the order of the vector.
            let malformed: BTreeSet<ActionId> = seen
                .iter()
                .filter(|(_, count)| **count > 1)
                .map(|(id, _)| *id)
                .collect();
            for id in &malformed {
                tracing::warn!(
                    %host,
                    action = %id,
                    "the agent answered more than once about a run; the answer is malformed and the run stays dispatched",
                );
            }
            let mut answered: BTreeSet<ActionId> = BTreeSet::new();
            for entry in answers.answers {
                if !asked.contains(&entry.execution) || malformed.contains(&entry.execution) {
                    continue;
                }
                answered.insert(entry.execution);
                match from_answer(entry.answer) {
                    Some(outcome) => queue(shared, writes, entry.execution, outcome),
                    None => running.push(entry.execution),
                }
            }
            for id in asked.difference(&answered) {
                if malformed.contains(id) {
                    // A malformed id is not asked about again in this pass: the agent has
                    // already shown it cannot answer this batch within the protocol.
                    continue;
                }
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
        repeats += 1;
        if repeats_exhausted(repeats) {
            tracing::warn!(
                %host,
                queries = repeats + 1,
                "dispatched runs are still running; reconciliation stops asking for now",
            );
            return;
        }
        if shared.deps.journal.epoch_of(host) != Some(epoch) {
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

/// The complete list of runs still `Dispatched` on `host`, or `None` when the store would
/// not finish listing them.
///
/// Each attempt holds the dispatch gate for the whole listing, so a dispatch that is
/// committed but not yet handed to the agent is never in the snapshot. A failed page starts
/// the next attempt at page one rather than joining pages from different attempts into a
/// list that would look complete. The gate is released before the pause and before any
/// query, so a retry never blocks a dispatch for long.
async fn snapshot(shared: &Shared, host: HostId) -> Option<Vec<ActionRequest>> {
    let mut attempt = 1;
    loop {
        let listed = {
            let _gate = shared.dispatch_gate(host).await;
            pending(shared, host).await
        };
        match listed {
            Ok(rows) => return Some(rows),
            Err(err) if attempt < RECONCILE_LISTING_ATTEMPTS => {
                tracing::warn!(
                    %err,
                    %host,
                    attempt,
                    "the store did not finish listing dispatched actions; starting over",
                );
                attempt += 1;
                let now = shared.deps.clock.now();
                let next = now.checked_add(RECONCILE_LISTING_BACKOFF).unwrap_or(now);
                shared.deps.clock.sleep_until(next).await;
            }
            Err(err) => {
                tracing::error!(
                    %err,
                    %host,
                    tries = attempt,
                    "the store would not list dispatched actions; nothing is reconciled this time",
                );
                return None;
            }
        }
    }
}

/// Every action still `Dispatched` on `host`, paged to the end. A short list is not a
/// complete list, so a failed page fails the whole listing.
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
/// guard. A refused write is logged by the writer and dropped: the row keeps whatever it
/// now holds.
///
/// The hand-over never awaits, so an answer that has already been received is committed even
/// when the session's replacement cancels this pass. If the writer queue is full the command
/// goes to one deferred worker instead of a task per answer, which keeps a large batch from
/// growing a task per row.
fn queue(
    shared: &Shared,
    writes: &mut Vec<oneshot::Receiver<Decision>>,
    id: ActionId,
    outcome: ReconcileOutcome,
) {
    if let Some(answer) = settle(shared, id, outcome) {
        writes.push(answer);
    }
}

/// Hands one settled outcome to the writer and answers with where its decision lands, when
/// the writer queue took it directly.
fn settle(
    shared: &Shared,
    id: ActionId,
    outcome: ReconcileOutcome,
) -> Option<oneshot::Receiver<Decision>> {
    let wire_reason = outcome.token();
    let (reply, answer) = oneshot::channel();
    let command = WriteCommand::Reconcile {
        id,
        outcome,
        wire_reason,
        reply,
    };
    match shared.writer.tx.try_send(command) {
        Ok(()) => Some(answer),
        Err(mpsc::error::TrySendError::Full(command)) => {
            if shared.deferred.send(command).is_err() {
                tracing::debug!(
                    action = %id,
                    "the action service is closing; the run stays dispatched",
                );
            }
            None
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::debug!(action = %id, "the action service is closing; the run stays dispatched");
            None
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
        ReconcileOutcome::HandoffFailed => {
            action
                .never_ran(HANDOFF_FAILED_REASON.to_owned(), now)
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
#[cfg(test)]
mod tests {
    use super::{RECONCILE_REPEATS, repeats_exhausted};

    #[test]
    fn a_pass_makes_one_query_then_the_bounded_repeats() {
        assert!(
            !repeats_exhausted(RECONCILE_REPEATS - 1),
            "the pass may make one more repeat query",
        );
        assert_eq!(
            RECONCILE_REPEATS, 240,
            "one initial query plus 240 repeats is 241 queries",
        );
        assert!(
            repeats_exhausted(RECONCILE_REPEATS),
            "the last permitted repeat query is followed by no more",
        );
        assert!(repeats_exhausted(RECONCILE_REPEATS + 1));
    }
}
