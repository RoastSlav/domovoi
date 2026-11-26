//! Settling dispatched targets from the agent's journal.
//!
//! A target the hub handed over and never heard back about stays `dispatched`. When the host's
//! agent authenticates, the hub asks that agent's journal what became of the execution and
//! settles the target with the answer: finished (with its result), not found (never started),
//! or unknown for the reasons the journal gives. A journal that was reset since the dispatch
//! cannot answer, so the target becomes an unknown outcome for a person to check.

use async_trait::async_trait;
use domovoi_core::install::{TargetFailure, codes};
use domovoi_core::ports::{HostReconciler, UnknownReason};
use domovoi_core::{HostId, InstallJob, InstallTarget, TargetStatus};

use super::deps::Shared;
use super::service::PackageHandle;
use super::settle::{self, Ending};
use crate::operations::reconcile_text::{
    NOT_RUN_REASON, ReconcileOutcome, from_answer, reason_text,
};

#[async_trait]
impl HostReconciler for PackageHandle {
    async fn reconcile_host(&self, host: HostId) {
        self.shared.wake.notify_one();
        host_targets(&self.shared, host).await;
    }
}

/// Reconciles every dispatched target of `host` that this process is not driving.
pub(super) async fn host_targets(shared: &Shared, host: HostId) {
    let targets = match shared.deps.installs.non_terminal_targets().await {
        Ok(targets) => targets,
        Err(err) => {
            tracing::warn!(%err, %host, "could not read the install targets to reconcile");
            return;
        }
    };
    for target in targets {
        if target.host != host || target.status != TargetStatus::Dispatched {
            continue;
        }
        // A target this process is driving settles from its own run; reading it as lost here
        // would race the send that follows its dispatch commit.
        if !shared.begin(host) {
            continue;
        }
        one(shared, &target).await;
        shared.end(host);
    }
}

/// Reconciles one dispatched target. The caller holds the host's mark.
pub(super) async fn one(shared: &Shared, target: &InstallTarget) {
    let job = match shared.deps.installs.job(target.job).await {
        Ok(Some(job)) => job,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(%err, target = %target.id, "could not read the job to reconcile");
            return;
        }
    };
    settle_from_journal(shared, &job, target).await;
}

/// After a run got no usable answer: asks the journal now, when the session's epoch is the one
/// the request went to.
pub(super) async fn after_uncertain(shared: &Shared, job: &InstallJob, target: &InstallTarget) {
    let host = target.host;
    if shared.deps.caps.journal.epoch_of(host) == target.dispatched_epoch {
        settle_from_journal(shared, job, target).await;
    }
}

/// The stored code of a journal reason.
const fn reason_code(reason: UnknownReason) -> &'static str {
    match reason {
        UnknownReason::NoStartRecord => "no_start_record",
        UnknownReason::AgentRestarted => "agent_restarted",
        UnknownReason::JournalLost => "journal_lost",
        UnknownReason::EpochChanged => codes::EPOCH_CHANGED,
        UnknownReason::EpochUnknown => codes::EPOCH_UNKNOWN,
    }
}

fn unknown(reason: UnknownReason) -> Ending {
    Ending::Unknown(TargetFailure::new(reason_code(reason), reason_text(reason)))
}

async fn settle_from_journal(shared: &Shared, job: &InstallJob, target: &InstallTarget) {
    let deps = &shared.deps;
    let host = target.host;
    let Some(recorded) = target.dispatched_epoch else {
        store(shared, job, target, unknown(UnknownReason::EpochUnknown)).await;
        return;
    };
    // No live session: the next authenticated hello asks again.
    let Some(live) = deps.caps.journal.epoch_of(host) else {
        return;
    };
    if live != recorded {
        store(shared, job, target, unknown(UnknownReason::EpochChanged)).await;
        return;
    }
    let now = deps.clock.now();
    let deadline = now.checked_add(shared.settings.result_grace).unwrap_or(now);
    let wanted = target.execution;
    let answers = match deps
        .caps
        .journal
        .status_batch(host, vec![wanted], recorded, deadline)
        .await
    {
        Ok(answers) => answers,
        Err(err) => {
            tracing::warn!(%err, %host, target = %target.id, "could not ask the agent about a package request");
            return;
        }
    };
    if answers.epoch != recorded {
        store(shared, job, target, unknown(UnknownReason::EpochChanged)).await;
        return;
    }
    let mut own = answers
        .answers
        .into_iter()
        .filter(|entry| entry.execution == wanted)
        .map(|entry| entry.answer);
    let first = own.next();
    // Two entries that disagree prove nothing; ask again later.
    let answer = match first {
        Some(first) if own.all(|next| next == first) => from_answer(first),
        _ => None,
    };
    let Some(settled) = answer else {
        return;
    };
    store(shared, job, target, ending(settled)).await;
}

fn ending(settled: ReconcileOutcome) -> Ending {
    match settled {
        ReconcileOutcome::Finished {
            succeeded: true, ..
        } => Ending::Succeeded {
            outputs: None,
            confirmed: false,
        },
        ReconcileOutcome::Finished {
            succeeded: false,
            detail,
        } => {
            let text = detail.unwrap_or_else(|| "the agent reports the request failed".to_owned());
            Ending::Failed(TargetFailure::from_agent_message(&text))
        }
        ReconcileOutcome::Unknown(reason) => unknown(reason),
        ReconcileOutcome::NeverRan => Ending::not_run(codes::NOT_RUN, NOT_RUN_REASON),
    }
}

async fn store(shared: &Shared, job: &InstallJob, target: &InstallTarget, ending: Ending) {
    if let Err(err) = settle::finish(shared, job, target, ending).await {
        tracing::debug!(?err, target = %target.id, "a reconciled target was not settled");
    }
}
