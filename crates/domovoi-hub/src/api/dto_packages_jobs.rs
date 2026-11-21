//! What the package endpoints return for jobs: each attempt on each host, and the status
//! derived from the latest of them.

use domovoi_core::InstallTarget;
use domovoi_core::package::PrivilegeBoundary;
use serde::Serialize;
use utoipa::ToSchema;

use super::dto_packages::kind_text;
use crate::services::JobView;

/// Why a host's target did not succeed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TargetFailureDto {
    /// A stable code such as `transfer_failed` or `signature_invalid`.
    pub code: String,
    /// What went wrong, in a sentence.
    pub message: String,
}

/// One attempt of a job on one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct InstallTargetDto {
    /// The target's id.
    pub id: String,
    /// The host's id.
    pub host_id: String,
    /// The host's name.
    pub host_name: String,
    /// Which attempt this is; a retry is the next one.
    pub attempt: u32,
    /// What approval of this host's work binds.
    pub plan_hash: String,
    /// `queued`, `transferring`, `dispatched`, `succeeded`, `failed`, `not_run`,
    /// `outcome_unknown` or `cancelled`.
    pub status: String,
    /// Why it did not succeed, when it did not.
    pub failure: Option<TargetFailureDto>,
    /// True when the agent's answer was read and matched; false when the result is from the
    /// agent's journal after a restart.
    pub outputs_confirmed: bool,
    /// When it was queued.
    pub queued_at: String,
    /// When the agent was handed the work.
    pub started_at: Option<String>,
    /// When it reached its final status.
    pub finished_at: Option<String>,
    /// When the owner's approval runs out.
    pub expires_at: String,
    /// `in_effect` or `not_in_effect`, when the agent has reported it for this package.
    pub privilege_boundary: Option<String>,
}

impl InstallTargetDto {
    fn of(view: &JobView, target: &InstallTarget) -> Self {
        Self {
            id: target.id.to_string(),
            host_id: target.host.to_string(),
            host_name: view
                .host_names
                .get(&target.host)
                .map_or_else(String::new, ToString::to_string),
            attempt: target.attempt,
            plan_hash: target.plan_hash.to_string(),
            status: target.status.as_str().to_owned(),
            failure: target.failure.as_ref().map(|failure| TargetFailureDto {
                code: failure.code().to_owned(),
                message: failure.message().to_owned(),
            }),
            outputs_confirmed: target.outputs_confirmed,
            queued_at: target.queued_at.to_string(),
            started_at: target.started_at.map(|at| at.to_string()),
            finished_at: target.finished_at.map(|at| at.to_string()),
            expires_at: target.expires_at.to_string(),
            privilege_boundary: view
                .boundaries
                .get(&target.host)
                .map(|boundary: &PrivilegeBoundary| boundary.as_str().to_owned()),
        }
    }
}

/// One install or uninstall job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct InstallJobDto {
    /// The job's id.
    pub id: String,
    /// `install` or `uninstall`.
    pub kind: String,
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: String,
    /// The archive's digest, for an install.
    pub archive_sha256: Option<String>,
    /// The signing key, for an install.
    pub key_id: Option<String>,
    /// The id of the account that approved it.
    pub requested_by: String,
    /// When it was created.
    pub created_at: String,
    /// `running`, `succeeded`, `failed` or `partial`, worked out from each host's latest
    /// attempt when the job is read.
    pub status: String,
    /// What the owner's confirmation bound.
    pub job_plan_hash: String,
    /// Every attempt on every host, oldest first.
    pub targets: Vec<InstallTargetDto>,
}

impl From<&JobView> for InstallJobDto {
    fn from(view: &JobView) -> Self {
        let job = &view.job;
        Self {
            id: job.id.to_string(),
            kind: kind_text(job.kind).to_owned(),
            name: job.name.to_string(),
            version: job.version.to_string(),
            archive_sha256: job.archive_sha256.as_ref().map(ToString::to_string),
            key_id: job.key_id.as_ref().map(ToString::to_string),
            requested_by: job.requested_by.to_string(),
            created_at: job.created_at.to_string(),
            status: view.status().as_str().to_owned(),
            job_plan_hash: job.job_plan_hash.to_string(),
            targets: view
                .targets
                .iter()
                .map(|target| InstallTargetDto::of(view, target))
                .collect(),
        }
    }
}

/// A page of jobs, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct JobPageDto {
    /// The jobs.
    pub items: Vec<InstallJobDto>,
    /// Pass as `cursor` for the next page; absent on the last.
    pub next_cursor: Option<String>,
}
