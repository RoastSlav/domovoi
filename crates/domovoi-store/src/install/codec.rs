//! Row shapes of the install tables and their conversions to the domain types.

use std::collections::BTreeMap;

use domovoi_core::install::{
    ArtifactSource, HostPackage, InstallJob, InstallKind, InstallTarget, ObservedBy,
    PackageArtifact, StoredAuthorization, TargetFailure, TargetStatus,
};
use domovoi_core::package::{KeyState, PackageStatus, PrivilegeBoundary};
use domovoi_core::ports::StoreError;
use domovoi_core::{Actor, ApprovalChannel, ApprovalProvenance, HumanApproval};

use crate::codec::{from_json, from_ms, from_ms_opt, from_name, json, ms, name, parse, uint};
use crate::error::corrupt;

/// Columns of `package_artifacts`, in [`ArtifactRow`] order.
macro_rules! artifact_columns {
    () => {
        "SELECT id, name, version, archive_sha256, key_id, \
    archive_size, signature_toml, signer_label, source, imported_by, imported_at \
    FROM package_artifacts"
    };
}
pub(crate) use artifact_columns;

/// One `package_artifacts` row.
#[derive(sqlx::FromRow)]
pub(crate) struct ArtifactRow {
    id: String,
    name: String,
    version: String,
    archive_sha256: String,
    key_id: String,
    archive_size: i64,
    signature_toml: String,
    signer_label: Option<String>,
    source: String,
    imported_by: String,
    imported_at: i64,
}

impl ArtifactRow {
    /// The domain artifact.
    pub(crate) fn into_artifact(self) -> Result<PackageArtifact, StoreError> {
        Ok(PackageArtifact {
            id: parse(&self.id, "artifact id")?,
            name: parse(&self.name, "package name")?,
            version: parse(&self.version, "package version")?,
            archive_sha256: parse(&self.archive_sha256, "archive digest")?,
            key_id: parse(&self.key_id, "key id")?,
            archive_size: uint(self.archive_size),
            signature_toml: self.signature_toml,
            signer_label: self.signer_label,
            source: ArtifactSource::parse(&self.source)
                .ok_or_else(|| corrupt("artifact source", &self.source))?,
            imported_by: parse::<Actor>(&self.imported_by, "importer")?,
            imported_at: from_ms(self.imported_at)?,
        })
    }
}

/// Columns of `install_jobs`, in [`JobRow`] order.
macro_rules! job_columns {
    () => {
        "SELECT id, kind, name, version, archive_sha256, key_id, \
    requested_by, created_at, job_plan_hash FROM install_jobs"
    };
}
pub(crate) use job_columns;

/// One `install_jobs` row.
#[derive(sqlx::FromRow)]
pub(crate) struct JobRow {
    id: String,
    kind: String,
    name: String,
    version: String,
    archive_sha256: Option<String>,
    key_id: Option<String>,
    requested_by: String,
    created_at: i64,
    job_plan_hash: String,
}

impl JobRow {
    /// The domain job.
    pub(crate) fn into_job(self) -> Result<InstallJob, StoreError> {
        Ok(InstallJob {
            id: parse(&self.id, "job id")?,
            kind: InstallKind::parse(&self.kind).ok_or_else(|| corrupt("job kind", &self.kind))?,
            name: parse(&self.name, "package name")?,
            version: parse(&self.version, "package version")?,
            archive_sha256: self
                .archive_sha256
                .as_deref()
                .map(|d| parse(d, "archive digest"))
                .transpose()?,
            key_id: self
                .key_id
                .as_deref()
                .map(|k| parse(k, "key id"))
                .transpose()?,
            requested_by: parse(&self.requested_by, "requester")?,
            created_at: from_ms(self.created_at)?,
            job_plan_hash: from_name(&self.job_plan_hash, "job plan hash")?,
        })
    }
}

/// Columns of `install_job_targets`, in [`TargetRow`] order.
macro_rules! target_columns {
    () => {
        "SELECT id, job_id, host_id, attempt, execution_id, \
    status, plan_hash, approved_by, approval_channel, approved_at, approval_session_ref, \
    approval_client, expires_at, artifact_transfer, signature_transfer, dispatched_session, \
    dispatched_epoch, failure_code, failure_message, outputs, outputs_confirmed, queued_at, \
    started_at, finished_at, version FROM install_job_targets"
    };
}
pub(crate) use target_columns;

/// One `install_job_targets` row.
#[derive(sqlx::FromRow)]
pub(crate) struct TargetRow {
    id: String,
    job_id: String,
    host_id: String,
    attempt: i64,
    execution_id: String,
    status: String,
    plan_hash: String,
    approved_by: String,
    approval_channel: String,
    approved_at: i64,
    approval_session_ref: Option<String>,
    approval_client: String,
    expires_at: i64,
    artifact_transfer: Option<String>,
    signature_transfer: Option<String>,
    dispatched_session: Option<String>,
    dispatched_epoch: Option<String>,
    failure_code: Option<String>,
    failure_message: Option<String>,
    outputs: Option<String>,
    outputs_confirmed: i64,
    queued_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    version: i64,
}

impl TargetRow {
    /// The domain target. The approval is rebuilt with `HumanApproval::restore`.
    pub(crate) fn into_target(self) -> Result<InstallTarget, StoreError> {
        let approval = HumanApproval::restore(
            parse(&self.approved_by, "approver")?,
            from_json::<ApprovalChannel>(&self.approval_channel, "approval channel")?,
            from_ms(self.approved_at)?,
        );
        let provenance = ApprovalProvenance::new(self.approval_session_ref, self.approval_client);
        let failure = match (self.failure_code, self.failure_message) {
            (Some(code), Some(message)) => Some(TargetFailure::new(&code, &message)),
            (None, None) => None,
            _ => return Err(corrupt("target failure", "partially stored")),
        };
        let outputs = self
            .outputs
            .as_deref()
            .map(|text| from_json::<BTreeMap<String, String>>(text, "target outputs"))
            .transpose()?;
        Ok(InstallTarget {
            id: parse(&self.id, "target id")?,
            job: parse(&self.job_id, "job id")?,
            host: parse(&self.host_id, "host id")?,
            attempt: u32::try_from(self.attempt).map_err(|e| corrupt("attempt", e))?,
            execution: parse(&self.execution_id, "execution id")?,
            status: TargetStatus::parse(&self.status)
                .ok_or_else(|| corrupt("target status", &self.status))?,
            plan_hash: from_name(&self.plan_hash, "plan hash")?,
            authorization: StoredAuthorization::new(approval, provenance),
            expires_at: from_ms(self.expires_at)?,
            artifact_transfer: self
                .artifact_transfer
                .as_deref()
                .map(|t| parse(t, "artifact transfer"))
                .transpose()?,
            signature_transfer: self
                .signature_transfer
                .as_deref()
                .map(|t| parse(t, "signature transfer"))
                .transpose()?,
            dispatched_session: self
                .dispatched_session
                .as_deref()
                .map(|s| parse(s, "dispatched session"))
                .transpose()?,
            dispatched_epoch: self
                .dispatched_epoch
                .as_deref()
                .map(|e| from_name(e, "dispatched epoch"))
                .transpose()?,
            failure,
            outputs,
            outputs_confirmed: self.outputs_confirmed != 0,
            queued_at: from_ms(self.queued_at)?,
            started_at: from_ms_opt(self.started_at)?,
            finished_at: from_ms_opt(self.finished_at)?,
            version: uint(self.version),
        })
    }
}

/// The values of one target row, encoded for binding.
pub(crate) struct TargetBinds {
    pub(crate) id: String,
    pub(crate) job_id: String,
    pub(crate) host_id: String,
    pub(crate) attempt: i64,
    pub(crate) execution_id: String,
    pub(crate) status: &'static str,
    pub(crate) plan_hash: String,
    pub(crate) approved_by: String,
    pub(crate) approval_channel: String,
    pub(crate) approved_at: i64,
    pub(crate) approval_session_ref: Option<String>,
    pub(crate) approval_client: String,
    pub(crate) expires_at: i64,
    pub(crate) artifact_transfer: Option<String>,
    pub(crate) signature_transfer: Option<String>,
    pub(crate) dispatched_session: Option<String>,
    pub(crate) dispatched_epoch: Option<String>,
    pub(crate) failure_code: Option<String>,
    pub(crate) failure_message: Option<String>,
    pub(crate) outputs: Option<String>,
    pub(crate) outputs_confirmed: i64,
    pub(crate) queued_at: i64,
    pub(crate) started_at: Option<i64>,
    pub(crate) finished_at: Option<i64>,
}

impl TargetBinds {
    /// Encodes `target` for an insert or update.
    pub(crate) fn of(target: &InstallTarget) -> Result<Self, StoreError> {
        let approval = target.authorization.approval();
        let provenance = target.authorization.provenance();
        Ok(Self {
            id: target.id.to_string(),
            job_id: target.job.to_string(),
            host_id: target.host.to_string(),
            attempt: i64::from(target.attempt),
            execution_id: target.execution.to_string(),
            status: target.status.as_str(),
            plan_hash: name(&target.plan_hash)?,
            approved_by: approval.user().to_string(),
            approval_channel: json(approval.channel())?,
            approved_at: ms(approval.at()),
            approval_session_ref: provenance.session_ref().map(str::to_owned),
            approval_client: provenance.client().to_owned(),
            expires_at: ms(target.expires_at),
            artifact_transfer: target.artifact_transfer.map(|t| t.to_string()),
            signature_transfer: target.signature_transfer.map(|t| t.to_string()),
            dispatched_session: target.dispatched_session.map(|s| s.to_string()),
            dispatched_epoch: target.dispatched_epoch.as_ref().map(name).transpose()?,
            failure_code: target.failure.as_ref().map(|f| f.code().to_owned()),
            failure_message: target.failure.as_ref().map(|f| f.message().to_owned()),
            outputs: target.outputs.as_ref().map(json).transpose()?,
            outputs_confirmed: i64::from(target.outputs_confirmed),
            queued_at: ms(target.queued_at),
            started_at: target.started_at.map(ms),
            finished_at: target.finished_at.map(ms),
        })
    }
}

/// Columns of `host_packages`, in [`PackageRow`] order.
macro_rules! package_columns {
    () => {
        "SELECT host_id, name, version, key_id, archive_sha256, \
    installed_at, status, key_state, skills, privilege_boundary, observed_at, observed_by \
    FROM host_packages"
    };
}
pub(crate) use package_columns;

/// One `host_packages` row.
#[derive(sqlx::FromRow)]
pub(crate) struct PackageRow {
    host_id: String,
    name: String,
    version: String,
    key_id: String,
    archive_sha256: String,
    installed_at: i64,
    status: String,
    key_state: String,
    skills: String,
    privilege_boundary: String,
    observed_at: i64,
    observed_by: String,
}

impl PackageRow {
    /// The domain inventory row.
    pub(crate) fn into_package(self) -> Result<HostPackage, StoreError> {
        Ok(HostPackage {
            host: parse(&self.host_id, "host id")?,
            name: parse(&self.name, "package name")?,
            version: parse(&self.version, "package version")?,
            key_id: parse(&self.key_id, "key id")?,
            archive_sha256: parse(&self.archive_sha256, "archive digest")?,
            installed_at: from_ms(self.installed_at)?,
            status: from_name::<PackageStatus>(&self.status, "package status")?,
            key_state: from_name::<KeyState>(&self.key_state, "key state")?,
            skills: from_json(&self.skills, "package skills")?,
            privilege_boundary: from_name::<PrivilegeBoundary>(
                &self.privilege_boundary,
                "privilege boundary",
            )?,
            observed_at: from_ms(self.observed_at)?,
            observed_by: ObservedBy::parse(&self.observed_by)
                .ok_or_else(|| corrupt("observed by", &self.observed_by))?,
        })
    }
}
