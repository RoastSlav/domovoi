//! Every change to a target is one transaction: the guarded target write, its journal row,
//! its audit rows and, for a finished install or uninstall, the inventory change. A change
//! whose guard no longer holds is dropped, never forced.

use std::collections::BTreeMap;

use domovoi_core::install::{TargetFailure, codes};
use domovoi_core::ports::{AuditDraft, CommitError, InstallWrite, StateWrite, Transaction};
use domovoi_core::{
    Actor, AgentSessionId, InstallJob, InstallKind, InstallTarget, JournalEpoch, TargetGuard,
    TargetStatus, UserId,
};
use jiff::Timestamp;
use serde_json::{Value, json};
use uuid::Uuid;

use super::audit;
use super::deps::Shared;
use super::inventory;

/// Why a change was not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SettleError {
    /// The target is no longer in the state the change expected; someone else moved it.
    Conflict,
    /// The store failed; nothing was written.
    Store(String),
}

/// How a target ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ending {
    /// Proof that nothing was installed or removed.
    NotRun(TargetFailure),
    /// The agent answered that it refused or failed.
    Failed(TargetFailure),
    /// The request was handed over and nobody can say what happened.
    Unknown(TargetFailure),
    /// The agent said it succeeded, but not with what the job asked for.
    Mismatch {
        /// The `result_mismatch` failure.
        failure: TargetFailure,
        /// What the agent reported.
        outputs: BTreeMap<String, String>,
    },
    /// The agent said it succeeded, or its journal says it finished ok.
    Succeeded {
        /// What the agent reported; `None` when only the journal said so.
        outputs: Option<BTreeMap<String, String>>,
        /// Whether the outputs came from the agent's own answer.
        confirmed: bool,
    },
}

impl Ending {
    /// A not-run ending with a stored code and message.
    pub(super) fn not_run(code: &str, message: &str) -> Self {
        Self::NotRun(TargetFailure::new(code, message))
    }

    /// The failure a not-run, failed or unknown ending carries.
    fn failure(&self) -> Option<&TargetFailure> {
        match self {
            Self::NotRun(f) | Self::Failed(f) | Self::Unknown(f) => Some(f),
            Self::Mismatch { failure, .. } => Some(failure),
            Self::Succeeded { .. } => None,
        }
    }

    fn status(&self) -> TargetStatus {
        match self {
            Self::NotRun(_) => TargetStatus::NotRun,
            Self::Failed(_) => TargetStatus::Failed,
            Self::Unknown(_) | Self::Mismatch { .. } => TargetStatus::OutcomeUnknown,
            Self::Succeeded { .. } => TargetStatus::Succeeded,
        }
    }
}

/// The ending of a target whose agent answered success with outputs that do not match.
pub(super) fn mismatch(outputs: BTreeMap<String, String>, detail: &str) -> Ending {
    Ending::Mismatch {
        failure: TargetFailure::new(codes::RESULT_MISMATCH, detail),
        outputs,
    }
}

/// Commits `tx` and maps what went wrong.
pub(super) async fn commit(shared: &Shared, tx: Transaction) -> Result<(), SettleError> {
    match shared.deps.uow.commit(tx).await {
        Ok(_) => Ok(()),
        Err(CommitError::Conflict { .. }) => Err(SettleError::Conflict),
        Err(err) => Err(SettleError::Store(err.to_string())),
    }
}

/// The guarded write of `next`, which replaces `current`.
fn write(current: &InstallTarget, next: &InstallTarget) -> StateWrite {
    StateWrite::Install(InstallWrite::Target {
        target: next.clone(),
        expected: TargetGuard {
            status: current.status,
            version: current.version,
        },
    })
}

/// Reads the target back after its change was stored.
async fn reread(shared: &Shared, current: &InstallTarget) -> Result<InstallTarget, SettleError> {
    match shared.deps.installs.target(current.id).await {
        Ok(Some(stored)) => Ok(stored),
        Ok(None) => Err(SettleError::Conflict),
        Err(err) => Err(SettleError::Store(err.to_string())),
    }
}

/// Queued becomes transferring: the worker owns the target and starts relaying.
pub(super) async fn claim(
    shared: &Shared,
    current: &InstallTarget,
) -> Result<InstallTarget, SettleError> {
    let now = shared.deps.clock.now();
    let mut next = current.clone();
    next.status = TargetStatus::Transferring;
    let tx = Transaction::new()
        .write(write(current, &next))
        .journal(audit::journal(
            current,
            Some(current.status),
            next.status,
            None,
            audit::worker(),
            now,
        ));
    commit(shared, tx).await?;
    reread(shared, current).await
}

/// The transfer ids and the session a request is handed over under.
pub(super) struct Handover {
    /// The agent session the request is for.
    pub(super) session: AgentSessionId,
    /// That session's journal epoch.
    pub(super) epoch: JournalEpoch,
    /// The transfer that carried the archive, for an install.
    pub(super) artifact_transfer: Option<Uuid>,
    /// The transfer that carried the signature file, for an install.
    pub(super) signature_transfer: Option<Uuid>,
}

/// Transferring becomes dispatched. Committed before the request is sent, so a crash after
/// the send leaves a target the agent's journal can be asked about.
pub(super) async fn dispatched(
    shared: &Shared,
    job: &InstallJob,
    current: &InstallTarget,
    handover: &Handover,
) -> Result<InstallTarget, SettleError> {
    let now = shared.deps.clock.now();
    let mut next = current.clone();
    next.status = TargetStatus::Dispatched;
    next.dispatched_session = Some(handover.session);
    next.dispatched_epoch = Some(handover.epoch);
    next.artifact_transfer = handover.artifact_transfer;
    next.signature_transfer = handover.signature_transfer;
    next.started_at = Some(now);
    let tx = Transaction::new()
        .write(write(current, &next))
        .journal(audit::journal(
            current,
            Some(current.status),
            next.status,
            None,
            audit::worker(),
            now,
        ))
        .audit(audit::event(
            audit::INSTALL_DISPATCHED,
            audit::worker(),
            now,
            job,
            &next,
            &[],
        ));
    commit(shared, tx).await?;
    reread(shared, current).await
}

/// Records that the archive and the signature file reached the agent. No state changes.
pub(super) async fn relayed(
    shared: &Shared,
    job: &InstallJob,
    target: &InstallTarget,
    artifact: Uuid,
    signature: Uuid,
) -> Result<(), SettleError> {
    let now = shared.deps.clock.now();
    let extra = [
        ("artifact_transfer", json!(artifact.to_string())),
        ("signature_transfer", json!(signature.to_string())),
    ];
    let tx = Transaction::new().audit(audit::event(
        audit::RELAYED,
        audit::worker(),
        now,
        job,
        target,
        &extra,
    ));
    commit(shared, tx).await
}

/// A queued target the owner cancelled.
pub(super) async fn cancelled(
    shared: &Shared,
    job: &InstallJob,
    current: &InstallTarget,
    user: UserId,
) -> Result<InstallTarget, SettleError> {
    let now = shared.deps.clock.now();
    let mut next = current.clone();
    next.status = TargetStatus::Cancelled;
    next.finished_at = Some(now);
    let actor = Actor::User(user);
    let tx = Transaction::new()
        .write(write(current, &next))
        .journal(audit::journal(
            current,
            Some(current.status),
            next.status,
            None,
            actor.clone(),
            now,
        ))
        .audit(audit::event(audit::CANCELLED, actor, now, job, &next, &[]));
    commit(shared, tx).await?;
    reread(shared, current).await
}

/// Ends a target with `ending`, and for a finished install or uninstall updates the host's
/// inventory in the same transaction.
pub(super) async fn finish(
    shared: &Shared,
    job: &InstallJob,
    current: &InstallTarget,
    ending: Ending,
) -> Result<InstallTarget, SettleError> {
    let now = shared.deps.clock.now();
    let mut next = current.clone();
    next.status = ending.status();
    next.failure = ending.failure().cloned();
    next.finished_at = Some(now);
    match &ending {
        Ending::Succeeded { outputs, confirmed } => {
            next.outputs.clone_from(outputs);
            next.outputs_confirmed = *confirmed;
        }
        Ending::Mismatch { outputs, .. } => {
            next.outputs = Some(outputs.clone());
            next.outputs_confirmed = true;
        }
        _ => {}
    }
    let code = next.failure.as_ref().map(|f| f.code().to_owned());
    let mut tx = Transaction::new()
        .write(write(current, &next))
        .journal(audit::journal(
            current,
            Some(current.status),
            next.status,
            code.as_deref(),
            audit::worker(),
            now,
        ));
    for row in audit_rows(job, &next, &ending, now) {
        tx = tx.audit(row);
    }
    if let Ending::Succeeded { outputs, .. } = &ending
        && let Some(change) = inventory_write(job, &next, outputs.as_ref(), now)
    {
        tx = tx.write(StateWrite::Install(change));
    }
    commit(shared, tx).await?;
    reread(shared, current).await
}

/// The audit rows an ending writes.
fn audit_rows(
    job: &InstallJob,
    target: &InstallTarget,
    ending: &Ending,
    now: Timestamp,
) -> Vec<AuditDraft> {
    let worker = audit::worker();
    let facts = |failure: &TargetFailure, key: &'static str| -> [(&'static str, Value); 2] {
        [
            (key, json!(failure.code())),
            ("message", json!(failure.message())),
        ]
    };
    match ending {
        Ending::Succeeded { confirmed, .. } => {
            let kind = match (job.kind, confirmed) {
                (InstallKind::Install, true) => audit::INSTALLED,
                (InstallKind::Install, false) => audit::CONFIRMED_BY_JOURNAL,
                (InstallKind::Uninstall, _) => audit::UNINSTALLED,
            };
            let extra = [("confirmed_by_agent_answer", json!(confirmed))];
            vec![audit::event(kind, worker, now, job, target, &extra)]
        }
        Ending::Failed(failure) => {
            let extra = facts(failure, "code");
            vec![audit::event(
                audit::REJECTED,
                worker,
                now,
                job,
                target,
                &extra,
            )]
        }
        Ending::NotRun(failure) => {
            let extra = facts(failure, "code");
            vec![audit::event(
                audit::NOT_RUN,
                worker,
                now,
                job,
                target,
                &extra,
            )]
        }
        Ending::Unknown(failure) => {
            let extra = facts(failure, "reason");
            vec![audit::event(
                audit::OUTCOME_UNKNOWN,
                worker,
                now,
                job,
                target,
                &extra,
            )]
        }
        Ending::Mismatch { failure, .. } => {
            let extra = facts(failure, "reason");
            vec![
                audit::event(audit::MISMATCH, worker.clone(), now, job, target, &extra),
                audit::event(audit::OUTCOME_UNKNOWN, worker, now, job, target, &extra),
            ]
        }
    }
}

/// The inventory change a successful job makes.
fn inventory_write(
    job: &InstallJob,
    target: &InstallTarget,
    outputs: Option<&BTreeMap<String, String>>,
    now: Timestamp,
) -> Option<InstallWrite> {
    match job.kind {
        InstallKind::Install => {
            let row = inventory::from_install(job, target, outputs?, now)?;
            Some(InstallWrite::Inventory {
                host: target.host,
                entries: vec![row],
                replace: false,
            })
        }
        InstallKind::Uninstall => Some(InstallWrite::RemoveInventory {
            host: target.host,
            name: job.name.clone(),
        }),
    }
}
