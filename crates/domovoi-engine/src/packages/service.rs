//! The install service: the handle that creates, retries and cancels jobs, and the task that
//! runs the worker.
//!
//! A command here reads current data, checks it against what the owner confirmed, and stores
//! the result in one transaction with its audit and journal rows. Nothing here talks to an
//! agent except [`PackageHandle::refresh_inventory`], through the same relay as the worker.

use std::sync::Arc;

use domovoi_core::ports::{CommitError, InstallWrite, StateWrite, Transaction};
use domovoi_core::{
    ApprovalProvenance, ExecutionId, HostId, HostPackage, HumanApproval, InstallJob, InstallJobId,
    InstallKind, InstallTarget, InstallTargetId, PlanHash, StoredAuthorization, TargetStatus,
    UserId,
};
use jiff::SignedDuration;
use tokio::task::JoinHandle;

use super::deps::{PackageDeps, PackageSettings, Shared};
use super::error::PackageError;
use super::plan::{self, InstallPlan, PlanRequest};
use super::settle::{self, SettleError};
use super::{audit, inventory, worker};

/// What an owner confirmed: the request and the job hash they saw for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRequest {
    /// What to do, to which package, on which hosts.
    pub plan: PlanRequest,
    /// The job plan hash the owner confirmed; creation fails when the hub's own is different.
    pub job_plan_hash: PlanHash,
}

/// The install service: the worker task and the handle around it.
#[derive(Debug)]
pub struct InstallService;

impl InstallService {
    /// Starts the worker task. At start, targets that were transferring when the hub stopped
    /// are settled as not run, and queued targets resume.
    #[must_use]
    pub fn spawn(deps: PackageDeps, settings: PackageSettings) -> (PackageHandle, JoinHandle<()>) {
        let shared = Arc::new(Shared::new(deps, settings));
        let task = tokio::spawn(worker::run(shared.clone()));
        (PackageHandle { shared }, task)
    }
}

/// The commands of the install service. Cheap to clone.
#[derive(Clone)]
pub struct PackageHandle {
    pub(super) shared: Arc<Shared>,
}

fn store(err: &impl ToString) -> PackageError {
    PackageError::Store(err.to_string())
}

impl PackageHandle {
    /// What `request` would do on each host now. Read-only.
    pub async fn plan(&self, request: &PlanRequest) -> Result<InstallPlan, PackageError> {
        plan::build(&self.shared, request).await
    }

    /// Changes how long confirmations created from now on stay usable.
    pub fn set_approval_window(&self, window: SignedDuration) {
        self.shared.set_approval_window(window);
    }

    /// Wakes the worker, for example when a host's agent authenticated.
    pub fn wake(&self) {
        self.shared.wake.notify_one();
    }

    /// Creates a job with one queued target per host, under the person's approval.
    ///
    /// Every host's plan is recomputed from current data, and the job is refused with
    /// [`PackageError::PlanChanged`] unless its hash is the one `request` carries.
    pub async fn create_job(
        &self,
        request: JobRequest,
        approval: HumanApproval,
        provenance: ApprovalProvenance,
    ) -> Result<(InstallJob, Vec<InstallTarget>), PackageError> {
        let shared = &*self.shared;
        let plan = plan::build(shared, &request.plan).await?;
        if plan.job_plan_hash != request.job_plan_hash {
            return Err(PackageError::PlanChanged);
        }
        let hosts: Vec<HostId> = request.plan.hosts.clone();
        if let Some(host) = busy_host(shared, &hosts).await? {
            return Err(PackageError::HostBusy(host));
        }
        let now = shared.deps.clock.now();
        let expires_at = now.checked_add(shared.approval_window()).unwrap_or(now);
        let job = InstallJob {
            id: InstallJobId::new(),
            kind: plan.kind,
            name: plan.name.clone(),
            version: plan.version.clone(),
            archive_sha256: plan.archive_sha256,
            key_id: plan.key_id.clone(),
            requested_by: approval.user(),
            created_at: now,
            job_plan_hash: plan.job_plan_hash,
        };
        let authorization = StoredAuthorization::new(approval.clone(), provenance);
        let targets: Vec<InstallTarget> = plan
            .hosts
            .iter()
            .map(|host| {
                new_target(
                    &job,
                    host.host,
                    1,
                    host.plan_hash,
                    &authorization,
                    (now, expires_at),
                )
            })
            .collect();
        let mut tx = Transaction::new().write(StateWrite::Install(InstallWrite::CreateJob {
            job: job.clone(),
            targets: targets.clone(),
        }));
        for target in &targets {
            tx = tx
                .audit(audit::requested(&job, target, approval.user(), now))
                .journal(audit::journal(
                    target,
                    None,
                    TargetStatus::Queued,
                    None,
                    domovoi_core::Actor::User(approval.user()),
                    now,
                ));
        }
        commit_new(shared, tx, &hosts).await?;
        self.wake();
        let stored = shared
            .deps
            .installs
            .targets(job.id)
            .await
            .map_err(|err| store(&err))?;
        Ok((job, stored))
    }

    /// Queues a new attempt on `host` for a target that did not succeed, under a new approval.
    ///
    /// Only the latest attempt, and only from failed, not run or unknown outcome. The host's
    /// plan hash is recomputed and must equal `plan_hash`.
    pub async fn retry_target(
        &self,
        job: InstallJobId,
        host: HostId,
        plan_hash: PlanHash,
        approval: HumanApproval,
        provenance: ApprovalProvenance,
    ) -> Result<InstallTarget, PackageError> {
        let shared = &*self.shared;
        let installs = &shared.deps.installs;
        let stored = installs
            .job(job)
            .await
            .map_err(|err| store(&err))?
            .ok_or(PackageError::NoSuchJob(job))?;
        let latest = installs
            .latest_target(job, host)
            .await
            .map_err(|err| store(&err))?
            .ok_or(PackageError::NoSuchTarget(host))?;
        if !matches!(
            latest.status,
            TargetStatus::Failed | TargetStatus::NotRun | TargetStatus::OutcomeUnknown
        ) {
            return Err(PackageError::Conflict(
                "only a failed, not run or unknown target can be retried".to_owned(),
            ));
        }
        if stored.kind == InstallKind::Install {
            let current = installs
                .artifact(&stored.name, &stored.version)
                .await
                .map_err(|err| store(&err))?;
            if current.map(|a| a.archive_sha256) != stored.archive_sha256 {
                return Err(PackageError::PlanChanged);
            }
        }
        if plan::current_hash(shared, &stored, host).await? != plan_hash {
            return Err(PackageError::PlanChanged);
        }
        if let Some(busy) = busy_host(shared, &[host]).await? {
            return Err(PackageError::HostBusy(busy));
        }
        let now = shared.deps.clock.now();
        let expires_at = now.checked_add(shared.approval_window()).unwrap_or(now);
        let authorization = StoredAuthorization::new(approval.clone(), provenance);
        let target = new_target(
            &stored,
            host,
            latest.attempt + 1,
            plan_hash,
            &authorization,
            (now, expires_at),
        );
        let tx = Transaction::new()
            .write(StateWrite::Install(InstallWrite::NewAttempt(
                target.clone(),
            )))
            .audit(audit::requested(&stored, &target, approval.user(), now))
            .journal(audit::journal(
                &target,
                None,
                TargetStatus::Queued,
                None,
                domovoi_core::Actor::User(approval.user()),
                now,
            ));
        commit_new(shared, tx, &[host]).await?;
        self.wake();
        installs
            .target(target.id)
            .await
            .map_err(|err| store(&err))?
            .ok_or_else(|| PackageError::Store("the new attempt was not stored".to_owned()))
    }

    /// Cancels the queued target of `job` on `host`. A target that already started cannot be
    /// cancelled.
    pub async fn cancel_target(
        &self,
        job: InstallJobId,
        host: HostId,
        actor: UserId,
    ) -> Result<InstallTarget, PackageError> {
        let shared = &*self.shared;
        let installs = &shared.deps.installs;
        let stored = installs
            .job(job)
            .await
            .map_err(|err| store(&err))?
            .ok_or(PackageError::NoSuchJob(job))?;
        let latest = installs
            .latest_target(job, host)
            .await
            .map_err(|err| store(&err))?
            .ok_or(PackageError::NoSuchTarget(host))?;
        let started =
            || PackageError::Conflict("already started; it cannot be cancelled".to_owned());
        if latest.status != TargetStatus::Queued {
            return Err(started());
        }
        match settle::cancelled(shared, &stored, &latest, actor).await {
            Ok(target) => Ok(target),
            Err(SettleError::Conflict) => Err(started()),
            Err(SettleError::Store(text)) => Err(PackageError::Store(text)),
        }
    }

    /// Lets a person turn the skills of a stored package on or off on `host`.
    pub async fn set_package_enabled(
        &self,
        host: HostId,
        actor: UserId,
        name: &domovoi_core::package::PackageName,
        version: &domovoi_core::package::PackageVersion,
        enable: bool,
    ) -> Result<inventory::EnableAnswer, PackageError> {
        inventory::set_enabled(&self.shared, host, actor, name, version, enable).await
    }

    /// Asks `host`'s agent what it has installed and replaces the hub's record of it. Only a
    /// person starts a refresh.
    pub async fn refresh_inventory(
        &self,
        host: HostId,
        actor: UserId,
    ) -> Result<Vec<HostPackage>, PackageError> {
        inventory::refresh(&self.shared, host, actor).await
    }
}

/// A queued target of `job` on `host`.
fn new_target(
    job: &InstallJob,
    host: HostId,
    attempt: u32,
    plan_hash: PlanHash,
    authorization: &StoredAuthorization,
    (queued_at, expires_at): (jiff::Timestamp, jiff::Timestamp),
) -> InstallTarget {
    InstallTarget {
        id: InstallTargetId::new(),
        job: job.id,
        host,
        attempt,
        execution: ExecutionId::new(),
        status: TargetStatus::Queued,
        plan_hash,
        authorization: authorization.clone(),
        expires_at,
        artifact_transfer: None,
        signature_transfer: None,
        dispatched_session: None,
        dispatched_epoch: None,
        failure: None,
        outputs: None,
        outputs_confirmed: false,
        queued_at,
        started_at: None,
        finished_at: None,
        version: 1,
    }
}

/// The first of `hosts` that has a target that has not finished.
async fn busy_host(shared: &Shared, hosts: &[HostId]) -> Result<Option<HostId>, PackageError> {
    let active = shared
        .deps
        .installs
        .non_terminal_targets()
        .await
        .map_err(|err| store(&err))?;
    Ok(hosts
        .iter()
        .find(|host| active.iter().any(|t| t.host == **host))
        .copied())
}

/// Commits a transaction that adds targets, telling a busy host from a store failure.
async fn commit_new(
    shared: &Shared,
    tx: Transaction,
    hosts: &[HostId],
) -> Result<(), PackageError> {
    match shared.deps.uow.commit(tx).await {
        Ok(_) => Ok(()),
        Err(CommitError::Constraint(text)) => match busy_host(shared, hosts).await? {
            Some(host) => Err(PackageError::HostBusy(host)),
            None => Err(PackageError::Conflict(text)),
        },
        Err(err) => Err(PackageError::Store(err.to_string())),
    }
}
