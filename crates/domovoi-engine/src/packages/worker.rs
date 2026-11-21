//! The worker: looks at every unfinished target, starts the ones that can start, and settles
//! the ones a restart or a store failure left behind.
//!
//! One pass reads the unfinished targets and starts at most one task per host: a queued
//! target is gated, claimed and run; a dispatched one that this process is not driving is
//! reconciled with the agent's journal. A pass runs at start, when the worker is woken (a job
//! was created, a host authenticated, a retry was queued) and every half minute, which is how
//! a target that waits for its host's agent notices the agent and how an approval that ran out
//! is settled.
//!
//! The worker is the only caller of the execution boundary: [`AuthorizedPackageTarget::of`]
//! and `execute::execute_package` each appear here once (EXEC-2).

use std::sync::Arc;

use domovoi_core::install::codes;
use domovoi_core::{InstallJob, InstallTarget, TargetStatus};
use jiff::Timestamp;

use super::deps::Shared;
use super::error::PackageExecError;
use super::execute::{AuthorizedPackageTarget, PackageArtifactFile};
use super::gate::{self, Gate};
use super::reconcile;
use super::settle::{self, Ending};

/// Runs the worker until the service is shut down.
pub(super) async fn run(shared: Arc<Shared>) {
    recover(&shared).await;
    loop {
        let wake = pass(&shared).await;
        tokio::select! {
            () = shared.deps.shutdown.cancelled() => return,
            () = shared.wake.notified() => {}
            () = shared.deps.clock.sleep_until(wake) => {}
        }
    }
}

/// What a started task does with its target.
#[derive(Clone, Copy)]
enum Work {
    /// Gate, claim and run a queued target.
    Run,
    /// Settle a target that was left transferring.
    Orphan,
    /// Ask the agent's journal about a dispatched target.
    Reconcile,
}

/// At start: a target that was transferring when the hub stopped was never handed over, so it
/// is settled as not run before the first pass.
async fn recover(shared: &Shared) {
    let targets = match shared.deps.installs.non_terminal_targets().await {
        Ok(targets) => targets,
        Err(err) => {
            tracing::error!(%err, "could not read the install targets at start");
            return;
        }
    };
    for target in targets {
        if target.status == TargetStatus::Transferring {
            orphan(shared, &target).await;
        }
    }
}

/// One look at every unfinished target. Returns when to look again.
async fn pass(shared: &Arc<Shared>) -> Timestamp {
    let now = shared.deps.clock.now();
    let mut wake = now
        .checked_add(shared.settings.wake_interval)
        .unwrap_or(now);
    let targets = match shared.deps.installs.non_terminal_targets().await {
        Ok(targets) => targets,
        Err(err) => {
            tracing::error!(%err, "could not read the install targets");
            return wake;
        }
    };
    for target in targets {
        let work = match target.status {
            TargetStatus::Queued => {
                if target.expires_at > now && target.expires_at < wake {
                    wake = target.expires_at;
                }
                Work::Run
            }
            TargetStatus::Transferring => Work::Orphan,
            TargetStatus::Dispatched => Work::Reconcile,
            _ => continue,
        };
        start(shared, target, work);
    }
    wake
}

/// Starts a task for `target` unless this process is already driving its host.
fn start(shared: &Arc<Shared>, target: InstallTarget, work: Work) {
    let host = target.host;
    if !shared.begin(host) {
        return;
    }
    let shared = shared.clone();
    tokio::spawn(async move {
        let changed = match work {
            Work::Run => run_target(&shared, target).await,
            Work::Orphan => orphan(&shared, &target).await,
            Work::Reconcile => {
                reconcile::one(&shared, &target).await;
                false
            }
        };
        shared.end(host);
        if changed {
            shared.wake.notify_one();
        }
    });
}

/// Settles a target nothing is driving that still says it is transferring.
async fn orphan(shared: &Shared, target: &InstallTarget) -> bool {
    let Some(job) = read_job(shared, target).await else {
        return false;
    };
    let ending = Ending::not_run(
        codes::HUB_RESTARTED,
        "the hub stopped while the files were being sent; nothing was installed",
    );
    settle::finish(shared, &job, target, ending).await.is_ok()
}

async fn read_job(shared: &Shared, target: &InstallTarget) -> Option<InstallJob> {
    match shared.deps.installs.job(target.job).await {
        Ok(job) => job,
        Err(err) => {
            tracing::warn!(%err, target = %target.id, "could not read the job of a target");
            None
        }
    }
}

/// The archive an install sends, from the index. An uninstall sends none.
async fn load_artifact(
    shared: &Shared,
    job: &InstallJob,
) -> Result<Option<PackageArtifactFile>, PackageExecError> {
    let Some(digest) = job.archive_sha256 else {
        return Ok(None);
    };
    let rows = shared
        .deps
        .installs
        .artifact_by_digest(&digest)
        .await
        .map_err(|err| PackageExecError::Store(err.to_string()))?;
    rows.into_iter()
        .find(|row| row.name == job.name && row.version == job.version)
        .map(|row| PackageArtifactFile {
            digest,
            size: row.archive_size,
            signature: row.signature_toml,
        })
        .map(Some)
        .ok_or_else(|| {
            PackageExecError::ArtifactMissing("the archive is no longer indexed".to_owned())
        })
}

/// Gates, claims and runs one queued target. `true` when its state changed.
async fn run_target(shared: &Shared, target: InstallTarget) -> bool {
    let Some(job) = read_job(shared, &target).await else {
        return false;
    };
    match gate::check(shared, &job, &target).await {
        Gate::Go => {}
        Gate::Wait => return false,
        Gate::Settle(err) => return settle_error(shared, &job, &target, &err).await,
    }
    let Ok(slot) = shared.slots.clone().acquire_owned().await else {
        return false;
    };
    let claimed = match settle::claim(shared, &target).await {
        Ok(claimed) => claimed,
        Err(err) => {
            tracing::debug!(?err, target = %target.id, "a queued target was not claimed");
            return false;
        }
    };
    let artifact = match load_artifact(shared, &job).await {
        Ok(artifact) => artifact,
        Err(err) => return settle_error(shared, &job, &claimed, &err).await,
    };
    let approval = claimed.authorization.approval().clone();
    let authorized = match AuthorizedPackageTarget::of(claimed.clone(), Some(approval)) {
        Ok(authorized) => authorized,
        Err(err) => return settle_error(shared, &job, &claimed, &err).await,
    };
    let sent =
        crate::packages::execute::execute_package(shared, authorized, &job, artifact, slot).await;
    match sent {
        Ok(outcome) => {
            tracing::debug!(target = %claimed.id, ?outcome, "a package target ran");
            true
        }
        Err(err) => settle_error(shared, &job, &claimed, &err).await,
    }
}

/// Settles `target` as not run for `err`, from whatever non-terminal state it is in now.
/// Errors that must not settle (superseded, store failure) are only logged.
async fn settle_error(
    shared: &Shared,
    job: &InstallJob,
    target: &InstallTarget,
    err: &PackageExecError,
) -> bool {
    let Some((code, message)) = err.not_run() else {
        tracing::warn!(%err, target = %target.id, "a package target stopped without settling");
        return false;
    };
    let current = match shared.deps.installs.target(target.id).await {
        Ok(Some(current)) if !current.status.is_terminal() => current,
        Ok(_) => return false,
        Err(err) => {
            tracing::warn!(%err, target = %target.id, "could not read a target to settle it");
            return false;
        }
    };
    let ending = Ending::not_run(code, &message);
    match settle::finish(shared, job, &current, ending).await {
        Ok(_) => true,
        Err(err) => {
            tracing::warn!(?err, target = %target.id, "a not-run settlement was not stored");
            false
        }
    }
}
