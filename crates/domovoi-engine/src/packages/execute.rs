//! The one path from an authorized install target to an agent.
//!
//! [`execute_package`] is the only function that relays package files and starts a package
//! capability for a job, and [`AuthorizedPackageTarget`] is what it takes: a value built from
//! a target that still carries the human approval a person gave it. The type is built once, by
//! the worker. This is also the only file of the engine that names the relay capability;
//! `xtask arch-check` (EXEC-2) and the engine's gate tests fail when anything else does.
//!
//! The procedure, in order, for a target that is already `transferring`:
//! 1. the approval window is still open;
//! 2. the live session is read once, so every frame and the request describe one session;
//! 3. an install streams the archive, then the signature file, each ending in a verdict;
//! 4. the session is checked again;
//! 5. the target is committed as `dispatched`, and only then is the request sent;
//! 6. the answer settles the target, or leaves it `dispatched` for the journal to settle.

use std::collections::BTreeMap;

use domovoi_core::install::TargetFailure;
use domovoi_core::package::{ArchiveDigest, keys};
use domovoi_core::ports::{AgentSnapshot, ArtifactReader, FileError, LinkError, PackageRelay};
use domovoi_core::{
    HumanApproval, InstallJob, InstallKind, InstallTarget, PackageCall, PackageDispatch,
    PackageOutputs, TargetStatus, TransferAnswer, TransferFrame, TransferKind,
};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;

use super::deps::Shared;
use super::error::PackageExecError;
use super::plan::contract_id;
use super::reconcile;
use super::relay::{self, Chunker, Mismatch};
use super::settle::{self, Ending, Handover, SettleError};

/// A target a person approved, as the one thing [`execute_package`] may run.
///
/// It exists for the length of one run: [`AuthorizedPackageTarget::of`] is its only
/// constructor, and it is never stored or serialised.
#[derive(Debug)]
pub(crate) struct AuthorizedPackageTarget {
    target: InstallTarget,
    approval: HumanApproval,
}

impl AuthorizedPackageTarget {
    /// The one constructor: it refuses a target with no human approval.
    pub(crate) fn of(
        target: InstallTarget,
        approval: Option<HumanApproval>,
    ) -> Result<Self, PackageExecError> {
        match approval {
            Some(approval) => Ok(Self { target, approval }),
            None => Err(PackageExecError::NotAuthorized),
        }
    }
}

/// What an install needs from the hub's store: which archive, how big, and the signature
/// file to send beside it.
#[derive(Debug, Clone)]
pub(crate) struct PackageArtifactFile {
    /// The digest the index records for the archive.
    pub(crate) digest: ArchiveDigest,
    /// The size the index records, in bytes.
    pub(crate) size: u64,
    /// The signature file's text.
    pub(crate) signature: String,
}

/// How a run ended, after the target was settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetOutcome {
    /// The target reached this terminal status.
    Settled(TargetStatus),
    /// The request was handed over and no answer came; the target stays dispatched for the
    /// agent's journal to settle.
    StillDispatched,
    /// Another writer moved the target first; nothing was settled here.
    Superseded,
}

fn relay(shared: &Shared) -> &dyn PackageRelay {
    &*shared.deps.caps.relay
}

/// Relays the files of an install (if any), hands the request to the agent and settles the
/// target from its answer.
///
/// `slot` limits how many hosts relay at once and is released as soon as the files are sent.
pub(crate) async fn execute_package(
    shared: &Shared,
    auth: AuthorizedPackageTarget,
    job: &InstallJob,
    artifact: Option<PackageArtifactFile>,
    slot: OwnedSemaphorePermit,
) -> Result<TargetOutcome, PackageExecError> {
    let AuthorizedPackageTarget { target, approval } = auth;
    tracing::debug!(target = %target.id, user = %approval.user(), "running an approved package target");
    let host = target.host;
    let deps = &shared.deps;
    if deps.clock.now() >= target.expires_at {
        return Err(PackageExecError::Expired);
    }
    let snapshot = deps
        .caps
        .observer
        .observe_snapshot(host)
        .ok_or(PackageExecError::HostUnavailable)?;
    let (call, transfers) = match job.kind {
        InstallKind::Install => {
            let file = artifact.ok_or_else(|| {
                PackageExecError::ArtifactMissing("no archive is indexed for this package".into())
            })?;
            let (artifact_transfer, signature_transfer) =
                send_files(shared, job, &target, &snapshot, &file).await?;
            let call = PackageCall::Install {
                package: job.name.clone(),
                version: job.version.clone(),
                artifact_transfer,
                signature_transfer,
            };
            (call, Some((artifact_transfer, signature_transfer)))
        }
        InstallKind::Uninstall => (
            PackageCall::Uninstall {
                package: job.name.clone(),
                version: job.version.clone(),
            },
            None,
        ),
    };
    drop(slot);
    recheck(shared, &target, &snapshot)?;
    let handover = Handover {
        session: snapshot.session,
        epoch: snapshot.journal_epoch,
        artifact_transfer: transfers.map(|t| t.0),
        signature_transfer: transfers.map(|t| t.1),
    };
    // The commit comes first: a crash after the send must leave a target the agent's journal
    // can be asked about.
    let dispatched = match settle::dispatched(shared, job, &target, &handover).await {
        Ok(stored) => stored,
        Err(SettleError::Conflict) => return Err(PackageExecError::Superseded),
        Err(SettleError::Store(text)) => return Err(PackageExecError::Store(text)),
    };
    let now = deps.clock.now();
    let deadline = answer_deadline(shared, job, now);
    let request = PackageDispatch {
        execution: dispatched.execution,
        call,
        plan_hash: dispatched.plan_hash,
        expires_at: dispatched.expires_at,
    };
    let handoff = relay(shared)
        .dispatch_package(
            host,
            request,
            snapshot.session,
            snapshot.journal_epoch,
            deadline,
        )
        .await
        .map_err(|err| match err {
            LinkError::SessionChanged => PackageExecError::SessionChanged,
            other => PackageExecError::HandoffFailed(cut(&other.to_string())),
        })?;
    let answer = relay(shared).package_result(host, handoff, deadline).await;
    settle_answer(shared, job, &dispatched, answer).await
}

/// Settles the dispatched target from the agent's answer.
async fn settle_answer(
    shared: &Shared,
    job: &InstallJob,
    dispatched: &InstallTarget,
    answer: Result<PackageOutputs, LinkError>,
) -> Result<TargetOutcome, PackageExecError> {
    let ending = match answer {
        Ok(PackageOutputs(outputs)) => match verify(job, &outputs) {
            Ok(()) => Ending::Succeeded {
                outputs: Some(outputs),
                confirmed: true,
            },
            Err(detail) => settle::mismatch(outputs, &detail),
        },
        Err(LinkError::Rejected(message)) => {
            Ending::Failed(TargetFailure::from_agent_message(&message))
        }
        Err(err) => {
            // The request was handed over and no usable answer came back. It may have run, so
            // it is neither a success nor a failure until the agent's journal says so.
            tracing::warn!(target = %dispatched.id, %err, "a package request's answer is unavailable");
            reconcile::after_uncertain(shared, job, dispatched).await;
            return Ok(TargetOutcome::StillDispatched);
        }
    };
    match settle::finish(shared, job, dispatched, ending).await {
        Ok(stored) => Ok(TargetOutcome::Settled(stored.status)),
        Err(SettleError::Conflict) => Ok(TargetOutcome::Superseded),
        Err(SettleError::Store(text)) => Err(PackageExecError::Store(text)),
    }
}

/// Whether the agent's success outputs say what the job asked for.
fn verify(job: &InstallJob, outputs: &BTreeMap<String, String>) -> Result<(), String> {
    let expect = |key: &str, value: String| -> Result<(), String> {
        match outputs.get(key) {
            Some(found) if *found == value => Ok(()),
            Some(_) => Err(format!("the agent's {key} is not the one asked for")),
            None => Err(format!("the agent's answer has no {key}")),
        }
    };
    expect(keys::OUT_PACKAGE, job.name.to_string())?;
    expect(keys::OUT_VERSION, job.version.to_string())?;
    if job.kind == InstallKind::Install {
        if let Some(key) = &job.key_id {
            expect(keys::OUT_KEY_ID, key.to_string())?;
        }
        if let Some(digest) = &job.archive_sha256 {
            expect(keys::OUT_ARCHIVE_SHA256, digest.to_string())?;
        }
        expect(keys::OUT_ENABLED, "false".to_owned())?;
    }
    Ok(())
}

/// When the answer to a started request stops being waited for: the contract's timeout and
/// a grace period after `now`.
fn answer_deadline(shared: &Shared, job: &InstallJob, now: Timestamp) -> Timestamp {
    let timeout = shared
        .contract(contract_id(job.kind))
        .map_or(SignedDuration::from_secs(300), |contract| contract.timeout);
    now.checked_add(timeout)
        .and_then(|at| at.checked_add(shared.settings.result_grace))
        .unwrap_or(now)
}

/// The session must be the one the files went to, and the approval must still be open.
fn recheck(
    shared: &Shared,
    target: &InstallTarget,
    snapshot: &AgentSnapshot,
) -> Result<(), PackageExecError> {
    let deps = &shared.deps;
    match deps.caps.observer.observe_snapshot(target.host) {
        Some(now)
            if now.session == snapshot.session && now.journal_epoch == snapshot.journal_epoch => {}
        _ => return Err(PackageExecError::SessionChanged),
    }
    if deps.clock.now() >= target.expires_at {
        return Err(PackageExecError::Expired);
    }
    Ok(())
}

/// Keeps agent and link text short enough to store.
fn cut(text: &str) -> String {
    text.chars()
        .take(domovoi_core::install::MAX_FAILURE_MESSAGE_CHARS)
        .collect()
}

fn transfer_failed(err: LinkError) -> PackageExecError {
    match err {
        LinkError::SessionChanged => PackageExecError::SessionChanged,
        other => PackageExecError::TransferFailed(cut(&other.to_string())),
    }
}

/// Sends one frame. Only an `End` is answered.
async fn send(
    shared: &Shared,
    host: domovoi_core::HostId,
    snapshot: &AgentSnapshot,
    deadline: Timestamp,
    frame: TransferFrame,
) -> Result<Option<TransferAnswer>, PackageExecError> {
    relay(shared)
        .relay_transfer(host, frame, snapshot.session, deadline)
        .await
        .map_err(transfer_failed)
}

/// Sends `End` and insists on the agent's `Stored`.
async fn finish_transfer(
    shared: &Shared,
    host: domovoi_core::HostId,
    snapshot: &AgentSnapshot,
    deadline: Timestamp,
    transfer: Uuid,
) -> Result<(), PackageExecError> {
    match send(
        shared,
        host,
        snapshot,
        deadline,
        TransferFrame::End { transfer },
    )
    .await?
    {
        Some(TransferAnswer::Stored) => Ok(()),
        Some(TransferAnswer::Failed { code, detail }) => Err(PackageExecError::TransferFailed(
            cut(&format!("{code}: {detail}")),
        )),
        None => Err(PackageExecError::TransferFailed(
            "the agent gave no answer to the transfer".to_owned(),
        )),
    }
}

fn corrupt(mismatch: &Mismatch) -> PackageExecError {
    PackageExecError::ArtifactCorrupt(mismatch.to_string())
}

fn unreadable(err: &FileError) -> PackageExecError {
    match err {
        FileError::NotFound => {
            PackageExecError::ArtifactMissing("the archive is not in the hub's store".to_owned())
        }
        FileError::Io(_) => {
            PackageExecError::ArtifactMissing("the archive could not be read".to_owned())
        }
    }
}

/// Relays the archive, then the signature file. Returns the two transfer ids.
async fn send_files(
    shared: &Shared,
    job: &InstallJob,
    target: &InstallTarget,
    snapshot: &AgentSnapshot,
    file: &PackageArtifactFile,
) -> Result<(Uuid, Uuid), PackageExecError> {
    let host = target.host;
    let mut reader = shared
        .deps
        .files
        .open(&file.digest)
        .await
        .map_err(|e| unreadable(&e))?;
    let artifact = Uuid::now_v7();
    stream_archive(shared, host, snapshot, artifact, file, &mut *reader).await?;
    let signature = Uuid::now_v7();
    send_signature(shared, host, snapshot, signature, &file.signature).await?;
    if let Err(err) = settle::relayed(shared, job, target, artifact, signature).await {
        tracing::warn!(?err, target = %target.id, "the relay audit row was not stored");
    }
    Ok((artifact, signature))
}

async fn stream_archive(
    shared: &Shared,
    host: domovoi_core::HostId,
    snapshot: &AgentSnapshot,
    transfer: Uuid,
    file: &PackageArtifactFile,
    reader: &mut dyn ArtifactReader,
) -> Result<(), PackageExecError> {
    let deadline = relay::transfer_deadline(shared.deps.clock.now(), file.size);
    let begin = relay::begin(
        transfer,
        TransferKind::PackageArtifact,
        file.size,
        *file.digest.as_bytes(),
    );
    send(shared, host, snapshot, deadline, begin).await?;
    let mut chunker = Chunker::new(transfer, file.size);
    while let Some(bytes) = reader.next_chunk().await.map_err(|e| unreadable(&e))? {
        let frame = chunker.push(bytes).map_err(|e| corrupt(&e))?;
        send(shared, host, snapshot, deadline, frame).await?;
    }
    // The totals are checked before `End`, so a file that changed on disk is never confirmed.
    chunker.finish(&file.digest).map_err(|e| corrupt(&e))?;
    finish_transfer(shared, host, snapshot, deadline, transfer).await
}

async fn send_signature(
    shared: &Shared,
    host: domovoi_core::HostId,
    snapshot: &AgentSnapshot,
    transfer: Uuid,
    text: &str,
) -> Result<(), PackageExecError> {
    let bytes = text.as_bytes();
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let deadline = relay::transfer_deadline(shared.deps.clock.now(), size);
    let begin = relay::begin(
        transfer,
        TransferKind::PackageManifest,
        size,
        relay::sha256_of(bytes),
    );
    send(shared, host, snapshot, deadline, begin).await?;
    for (seq, piece) in relay::pieces(bytes).into_iter().enumerate() {
        let frame = TransferFrame::Chunk {
            transfer,
            seq: u64::try_from(seq).unwrap_or(u64::MAX),
            bytes: piece,
        };
        send(shared, host, snapshot, deadline, frame).await?;
    }
    finish_transfer(shared, host, snapshot, deadline, transfer).await
}

/// Hands an `agent.package.list` request to the host and waits for its answer. A person starts
/// every list; nothing in the background calls this.
pub(super) async fn list_packages(
    shared: &Shared,
    host: domovoi_core::HostId,
    snapshot: &AgentSnapshot,
    request: PackageDispatch,
    deadline: Timestamp,
) -> Result<PackageOutputs, LinkError> {
    let handoff = relay(shared)
        .dispatch_package(
            host,
            request,
            snapshot.session,
            snapshot.journal_epoch,
            deadline,
        )
        .await?;
    relay(shared).package_result(host, handoff, deadline).await
}
