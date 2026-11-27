//! Install jobs: planning, creating, retrying and cancelling them, reading them back, and a
//! host's inventory.
//!
//! The step-up order of a create or a retry is fixed (ADR 0029): packages enabled, request
//! shape, the password, the typed package name, the plan hash, and only then the approval.
//! A refusal at any step creates nothing.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::{InstallJobFilter, Page, Paged};
use domovoi_core::{HostId, HostPackage, InstallJob, InstallJobId, InstallKind, PlanHash};
use domovoi_engine::packages::{InstallPlan, JobRequest, PackageError, PlanRequest};

use super::view::JobView;
use super::{PackageCommandError, PackageCommands};
use crate::auth::approval::web_package_authorization;
use crate::services::{Caller, StepUp};

/// The most hosts one job targets.
pub const MAX_JOB_HOSTS: usize = 16;

/// What a plan or a job is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInput {
    /// Install or uninstall.
    pub kind: InstallKind,
    /// The package.
    pub name: PackageName,
    /// Its version.
    pub version: PackageVersion,
    /// The hosts, one to sixteen, each once.
    pub hosts: Vec<HostId>,
}

/// A request to create a job: what was planned, the plan hash the owner confirmed and the
/// package name they typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInput {
    /// What the job does and where.
    pub plan: PlanInput,
    /// The job plan hash from the plan the owner saw.
    pub plan_hash: PlanHash,
    /// The text the owner typed; it must be the package name.
    pub confirm_text: String,
}

/// A request to run one host's target of a job again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryInput {
    /// The job.
    pub job: InstallJobId,
    /// The host.
    pub host: HostId,
    /// The host's plan hash from the plan the owner saw.
    pub plan_hash: PlanHash,
    /// The text the owner typed; it must be the package name.
    pub confirm_text: String,
}

/// Which jobs to list.
#[derive(Debug, Clone, Default)]
pub struct JobsQuery {
    /// Only jobs with a target on this host.
    pub host: Option<HostId>,
    /// Only jobs for this package.
    pub name: Option<PackageName>,
    /// `Some(true)` for jobs with an unfinished target, `Some(false)` for finished ones.
    pub active: Option<bool>,
    /// Which page.
    pub page: Option<Page>,
}

fn request_of(input: &PlanInput) -> PlanRequest {
    PlanRequest {
        kind: input.kind,
        name: input.name.clone(),
        version: input.version.clone(),
        hosts: input.hosts.clone(),
    }
}

/// Refuses a host list the install service would refuse, before a password is asked for.
fn check_shape(hosts: &[HostId]) -> Result<(), PackageCommandError> {
    if hosts.is_empty() || hosts.len() > MAX_JOB_HOSTS {
        return Err(PackageCommandError::BadRequest(format!(
            "a job needs between 1 and {MAX_JOB_HOSTS} hosts"
        )));
    }
    let distinct: BTreeSet<_> = hosts.iter().collect();
    if distinct.len() != hosts.len() {
        return Err(PackageCommandError::BadRequest(
            "a host is named twice".to_owned(),
        ));
    }
    Ok(())
}

fn confirm(typed: &str, name: &PackageName) -> Result<(), PackageCommandError> {
    if typed == name.as_str() {
        Ok(())
    } else {
        Err(PackageCommandError::ConfirmationMismatch)
    }
}

impl PackageCommands {
    /// What a request would do on each host, bound to the hashes an approval covers. Reads
    /// only; it works while packages are disabled.
    ///
    /// # Errors
    /// The install service's refusal: no such package, an unknown host.
    pub async fn plan(&self, input: &PlanInput) -> Result<InstallPlan, PackageCommandError> {
        check_shape(&input.hosts)?;
        Ok(self.handle.plan(&request_of(input)).await?)
    }

    /// Creates a job with one target per host.
    ///
    /// # Errors
    /// In order: packages disabled, a malformed request, a missing or wrong password, a typed
    /// text that is not the package name, a plan that changed since the owner saw it, then
    /// whatever the install service refuses (a busy host).
    pub async fn create_job(
        &self,
        caller: &Caller,
        input: JobInput,
        proof: StepUp<'_>,
    ) -> Result<JobView, PackageCommandError> {
        let session = Self::web_session(caller)?;
        self.require_enabled()?;
        check_shape(&input.plan.hosts)?;
        self.step_up(caller, proof).await?;
        confirm(&input.confirm_text, &input.plan.name)?;
        let request = request_of(&input.plan);
        let plan = self.handle.plan(&request).await?;
        if plan.job_plan_hash != input.plan_hash {
            return Err(PackageError::PlanChanged.into());
        }
        let (approval, provenance) =
            web_package_authorization(caller.user(), session, self.clock_now());
        let (job, _) = self
            .handle
            .create_job(
                JobRequest {
                    plan: request,
                    job_plan_hash: input.plan_hash,
                },
                approval,
                provenance,
            )
            .await?;
        self.view(job).await
    }

    /// Turns the skills of the package stored on `host` on or off. Never automatic: a person
    /// asks, with the password again and the package name typed out. The agent re-verifies the
    /// files; the hub audits the decision with the plan hash.
    ///
    /// # Errors
    /// Packages disabled, a missing or wrong password, a typed text that is not the package
    /// name, or the install service's refusal (agent not connected, refused by the agent).
    pub async fn set_skills_enabled(
        &self,
        caller: &Caller,
        host: HostId,
        name: &PackageName,
        version: &domovoi_core::package::PackageVersion,
        enable: bool,
        confirm_text: &str,
        proof: StepUp<'_>,
    ) -> Result<domovoi_engine::packages::EnableAnswer, PackageCommandError> {
        Self::web_session(caller)?;
        self.require_enabled()?;
        self.step_up(caller, proof).await?;
        confirm(confirm_text, name)?;
        Ok(self
            .handle
            .set_package_enabled(host, caller.user(), name, version, enable)
            .await?)
    }

    /// Runs one host's target of a job again as a new attempt.
    ///
    /// # Errors
    /// The same order as [`create_job`](Self::create_job); the install service checks the
    /// host's plan hash and that the latest attempt is in a state that can be retried.
    pub async fn retry(
        &self,
        caller: &Caller,
        input: RetryInput,
        proof: StepUp<'_>,
    ) -> Result<JobView, PackageCommandError> {
        let session = Self::web_session(caller)?;
        self.require_enabled()?;
        self.step_up(caller, proof).await?;
        let job = self.job_row(input.job).await?;
        confirm(&input.confirm_text, &job.name)?;
        let (approval, provenance) =
            web_package_authorization(caller.user(), session, self.clock_now());
        self.handle
            .retry_target(input.job, input.host, input.plan_hash, approval, provenance)
            .await?;
        self.view(job).await
    }

    /// Cancels a target that has not started. Cancelling only ever removes work, so it stays
    /// possible while packages are disabled.
    ///
    /// # Errors
    /// No such job or host, or the target already started.
    pub async fn cancel(
        &self,
        caller: &Caller,
        job: InstallJobId,
        host: HostId,
    ) -> Result<JobView, PackageCommandError> {
        Self::web_session(caller)?;
        let row = self.job_row(job).await?;
        self.handle.cancel_target(job, host, caller.user()).await?;
        self.view(row).await
    }

    /// One job with its targets.
    ///
    /// # Errors
    /// No such job, or a store failure.
    pub async fn job(&self, id: InstallJobId) -> Result<JobView, PackageCommandError> {
        let row = self.job_row(id).await?;
        self.view(row).await
    }

    /// A page of jobs, newest first.
    ///
    /// # Errors
    /// A store failure.
    pub async fn jobs(&self, query: JobsQuery) -> Result<Paged<JobView>, PackageCommandError> {
        let filter = InstallJobFilter {
            host: query.host,
            name: query.name,
            active: query.active,
        };
        let page = query.page.unwrap_or_else(|| Page::first(50));
        let found = self.installs.jobs(&filter, &page).await?;
        let mut items = Vec::with_capacity(found.items.len());
        for job in found.items {
            items.push(self.view(job).await?);
        }
        Ok(Paged {
            items,
            next_cursor: found.next_cursor,
        })
    }

    /// What the hub last saw installed on `host`.
    ///
    /// # Errors
    /// A store failure.
    pub async fn host_packages(
        &self,
        host: HostId,
    ) -> Result<Vec<HostPackage>, PackageCommandError> {
        Ok(self.installs.inventory(host).await?)
    }

    /// Asks `host`'s agent which packages it has and replaces the stored inventory with the
    /// answer.
    ///
    /// # Errors
    /// Packages disabled, or the install service's refusal: the agent is not connected or its
    /// answer is unreadable.
    pub async fn refresh(
        &self,
        caller: &Caller,
        host: HostId,
    ) -> Result<Vec<HostPackage>, PackageCommandError> {
        Self::web_session(caller)?;
        self.require_enabled()?;
        Ok(self.handle.refresh_inventory(host, caller.user()).await?)
    }

    async fn job_row(&self, id: InstallJobId) -> Result<InstallJob, PackageCommandError> {
        self.installs
            .job(id)
            .await?
            .ok_or_else(|| PackageError::NoSuchJob(id).into())
    }

    /// The job with its targets, the names of their hosts and the privilege boundary each
    /// host reported for the package.
    async fn view(&self, job: InstallJob) -> Result<JobView, PackageCommandError> {
        let targets = self.installs.targets(job.id).await?;
        let mut host_names = BTreeMap::new();
        let mut boundaries = BTreeMap::new();
        for target in &targets {
            if host_names.contains_key(&target.host) {
                continue;
            }
            if let Some(host) = self.hosts.get_host(target.host).await? {
                host_names.insert(target.host, host.name);
            }
            let held = self.installs.inventory(target.host).await?;
            if let Some(row) = held.into_iter().find(|row| row.name == job.name) {
                boundaries.insert(target.host, row.privilege_boundary);
            }
        }
        Ok(JobView {
            job,
            targets,
            host_names,
            boundaries,
        })
    }
}
