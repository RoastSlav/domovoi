//! Applying the install aggregates' writes to in-memory state, with the same guards as the
//! SQLite tables: one active target per host, unique name and version, guarded target writes
//! and no deleting an artifact a running job uses.

use domovoi_core::install::{ArtifactGuard, InstallJob, InstallTarget, TargetGuard};
use domovoi_core::ports::{AggregateRef, CommitError, Expectation, InstallWrite, Observed};
use domovoi_core::{HostId, PackageArtifactId};

use super::State;

fn refuse(message: String) -> CommitError {
    CommitError::Constraint(message)
}

/// Stores one install write.
pub(super) fn apply(state: &mut State, write: &InstallWrite) -> Result<(), CommitError> {
    match write {
        InstallWrite::ImportArtifact(artifact) => {
            let taken = state
                .install
                .artifacts
                .values()
                .any(|a| a.name == artifact.name && a.version == artifact.version);
            if taken || state.install.artifacts.contains_key(&artifact.id) {
                return Err(refuse(format!(
                    "{}@{} is already imported",
                    artifact.name, artifact.version
                )));
            }
            state
                .install
                .artifacts
                .insert(artifact.id, artifact.clone());
            Ok(())
        }
        InstallWrite::DeleteArtifact { id, expected } => delete_artifact(state, *id, expected),
        InstallWrite::CreateJob { job, targets } => create_job(state, job, targets),
        InstallWrite::NewAttempt(target) => insert_target(state, target),
        InstallWrite::Target { target, expected } => update_target(state, target, *expected),
        InstallWrite::Inventory {
            host,
            entries,
            replace,
        } => {
            if entries.iter().any(|entry| entry.host != *host) {
                return Err(refuse(
                    "an inventory row belongs to another host".to_owned(),
                ));
            }
            for entry in entries {
                state
                    .install
                    .packages
                    .insert((*host, entry.name.clone()), entry.clone());
            }
            if *replace {
                state.install.packages.retain(|(h, name), _| {
                    *h != *host || entries.iter().any(|entry| entry.name == *name)
                });
            }
            Ok(())
        }
        InstallWrite::RemoveInventory { host, name } => {
            state.install.packages.remove(&(*host, name.clone()));
            Ok(())
        }
    }
}

fn delete_artifact(
    state: &mut State,
    id: PackageArtifactId,
    expected: &ArtifactGuard,
) -> Result<(), CommitError> {
    let Some(artifact) = state.install.artifacts.get(&id) else {
        return Err(refuse("the artifact does not exist".to_owned()));
    };
    if artifact.archive_sha256 != expected.archive_sha256 {
        return Err(refuse("the artifact changed since it was read".to_owned()));
    }
    let in_use = state.install.jobs.values().any(|job| {
        job.name == artifact.name
            && job.version == artifact.version
            && job.archive_sha256 == Some(artifact.archive_sha256)
            && state
                .install
                .targets
                .values()
                .any(|t| t.job == job.id && !t.status.is_terminal())
    });
    if in_use {
        return Err(refuse(format!(
            "{}@{} is used by a running install job",
            artifact.name, artifact.version
        )));
    }
    state.install.artifacts.remove(&id);
    Ok(())
}

fn create_job(
    state: &mut State,
    job: &InstallJob,
    targets: &[InstallTarget],
) -> Result<(), CommitError> {
    if targets.is_empty() {
        return Err(refuse("a job needs at least one target".to_owned()));
    }
    if targets.iter().any(|target| target.job != job.id) {
        return Err(refuse("a target belongs to another job".to_owned()));
    }
    if state.install.jobs.contains_key(&job.id) {
        return Err(refuse("the job already exists".to_owned()));
    }
    state.install.jobs.insert(job.id, job.clone());
    for target in targets {
        insert_target(state, target)?;
    }
    Ok(())
}

/// Whether `host` has a queued, transferring or dispatched target.
fn host_busy(state: &State, host: HostId) -> bool {
    state
        .install
        .targets
        .values()
        .any(|t| t.host == host && !t.status.is_terminal())
}

fn insert_target(state: &mut State, target: &InstallTarget) -> Result<(), CommitError> {
    if !state.install.jobs.contains_key(&target.job) {
        return Err(refuse("unknown install job".to_owned()));
    }
    if state.install.targets.contains_key(&target.id) {
        return Err(refuse("the target already exists".to_owned()));
    }
    let duplicate = state.install.targets.values().any(|t| {
        t.execution == target.execution
            || (t.job == target.job && t.host == target.host && t.attempt == target.attempt)
    });
    if duplicate {
        return Err(refuse("the execution or attempt already exists".to_owned()));
    }
    if !target.status.is_terminal() && host_busy(state, target.host) {
        return Err(refuse(format!(
            "host {} already has an install target in progress",
            target.host
        )));
    }
    let mut stored = target.clone();
    stored.version = 1;
    state.install.targets.insert(stored.id, stored);
    Ok(())
}

fn update_target(
    state: &mut State,
    target: &InstallTarget,
    guard: TargetGuard,
) -> Result<(), CommitError> {
    let conflict = |expected: Expectation, found: Observed| CommitError::Conflict {
        aggregate: AggregateRef::install_job(target.job),
        expected,
        found,
    };
    let Some(stored) = state.install.targets.get_mut(&target.id) else {
        return Err(conflict(
            Expectation::Version(guard.version),
            Observed::Missing,
        ));
    };
    if stored.status != guard.status {
        return Err(conflict(
            Expectation::Status(guard.status.as_str().to_owned()),
            Observed::Status(stored.status.as_str().to_owned()),
        ));
    }
    if stored.version != guard.version {
        return Err(conflict(
            Expectation::Version(guard.version),
            Observed::Version(stored.version),
        ));
    }
    // Only the mutable columns change, as in SQL.
    stored.status = target.status;
    stored.artifact_transfer = target.artifact_transfer;
    stored.signature_transfer = target.signature_transfer;
    stored.dispatched_session = target.dispatched_session;
    stored.dispatched_epoch = target.dispatched_epoch;
    stored.failure = target.failure.clone();
    stored.outputs = target.outputs.clone();
    stored.outputs_confirmed = target.outputs_confirmed;
    stored.started_at = target.started_at;
    stored.finished_at = target.finished_at;
    stored.version = guard.version + 1;
    Ok(())
}
