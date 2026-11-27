//! What the package endpoints take and return. Nothing here carries a secret, a path on the
//! hub's disk or a package's contents, and no request has a field for an approval: the hub
//! makes that itself after the password and the typed name check out (ADR 0029).

use domovoi_core::install::{InstallKind, ObservedBy};
use domovoi_core::package::{KeyState, PackageStatus};
use domovoi_core::{HostPackage, PackageArtifact};
use domovoi_engine::packages::{HostPlan, InstallPlan};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::services::{PackageDetail, PackagesStatus};

/// An owner key the hub accepts signatures from. The key itself is never returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OwnerKeyDto {
    /// The key id: 16 lowercase hex characters.
    pub id: String,
    /// The owner's note about the key.
    pub label: Option<String>,
}

/// What the hub holds, how it is limited and which keys it trusts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PackagesStatusDto {
    /// Whether packages can be imported and installed.
    pub enabled: bool,
    /// The largest archive accepted, in bytes.
    pub max_artifact_bytes: u64,
    /// The most bytes of archives the hub keeps.
    pub max_store_bytes: u64,
    /// Bytes of archives the hub holds now.
    pub used_store_bytes: u64,
    /// How many packages are imported.
    pub artifact_count: u64,
    /// The keys whose signatures the hub accepts.
    pub owner_keys: Vec<OwnerKeyDto>,
    /// Key ids that are never accepted.
    pub revoked: Vec<String>,
    /// How long an approval to install stays valid.
    pub approval_window_secs: i64,
}

impl From<PackagesStatus> for PackagesStatusDto {
    fn from(status: PackagesStatus) -> Self {
        Self {
            enabled: status.enabled,
            max_artifact_bytes: status.max_artifact_bytes,
            max_store_bytes: status.max_store_bytes,
            used_store_bytes: status.used_store_bytes,
            artifact_count: status.artifact_count,
            owner_keys: status
                .owner_keys
                .into_iter()
                .map(|(id, label)| OwnerKeyDto { id, label })
                .collect(),
            revoked: status.revoked,
            approval_window_secs: status.approval_window_secs,
        }
    }
}

/// One imported package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PackageDto {
    /// The package name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// SHA-256 of the archive: 64 lowercase hex characters.
    pub archive_sha256: String,
    /// The key that signed it: 16 lowercase hex characters.
    pub key_id: String,
    /// The archive's size in bytes.
    pub archive_size: u64,
    /// The label of the signing key in the hub's owner keys, if it has one.
    pub signer_label: Option<String>,
    /// When it was imported.
    pub imported_at: String,
    /// True when an import found this exact archive already imported and changed nothing.
    pub already_imported: bool,
}

impl PackageDto {
    /// The DTO of `artifact`.
    #[must_use]
    pub fn of(artifact: &PackageArtifact, already_imported: bool) -> Self {
        Self {
            name: artifact.name.to_string(),
            version: artifact.version.to_string(),
            archive_sha256: artifact.archive_sha256.to_string(),
            key_id: artifact.key_id.to_string(),
            archive_size: artifact.archive_size,
            signer_label: artifact.signer_label.clone(),
            imported_at: artifact.imported_at.to_string(),
            already_imported,
        }
    }
}

/// A host's copy of a package, as the hub last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PackageHostDto {
    /// The host's id.
    pub host_id: String,
    /// The host's name.
    pub host_name: String,
    /// `active`, `disabled_untrusted_key` or `missing_files`.
    pub status: String,
    /// When the hub saw it.
    pub observed_at: String,
}

/// One imported package with the hosts that have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PackageDetailDto {
    /// The package.
    #[serde(flatten)]
    pub package: PackageDto,
    /// The signature file's text, as imported.
    pub signature_toml: String,
    /// The hosts known to have this version.
    pub hosts: Vec<PackageHostDto>,
    /// `ready`, or `missing` when the archive file is not in the store and the package must
    /// be imported again before it can be installed.
    pub file_status: String,
}

impl From<PackageDetail> for PackageDetailDto {
    fn from(detail: PackageDetail) -> Self {
        Self {
            package: PackageDto::of(&detail.artifact, false),
            signature_toml: detail.artifact.signature_toml,
            hosts: detail
                .hosts
                .into_iter()
                .map(|(row, name)| PackageHostDto {
                    host_id: row.host.to_string(),
                    host_name: name.to_string(),
                    status: status_text(row.status).to_owned(),
                    observed_at: row.observed_at.to_string(),
                })
                .collect(),
            file_status: if detail.file_present {
                "ready"
            } else {
                "missing"
            }
            .to_owned(),
        }
    }
}

const fn status_text(status: PackageStatus) -> &'static str {
    match status {
        PackageStatus::Active => "active",
        PackageStatus::DisabledUntrustedKey => "disabled_untrusted_key",
        PackageStatus::MissingFiles => "missing_files",
    }
}

const fn key_state_text(state: KeyState) -> &'static str {
    match state {
        KeyState::Trusted => "trusted",
        KeyState::Unknown => "unknown",
        KeyState::Revoked => "revoked",
    }
}

/// A package installed on a host, as the agent last reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HostPackageDto {
    /// The package name.
    pub name: String,
    /// The installed version.
    pub version: String,
    /// The key that signed it.
    pub key_id: String,
    /// SHA-256 of the installed archive.
    pub archive_sha256: String,
    /// When the agent installed it.
    pub installed_at: String,
    /// `active`, `disabled_untrusted_key` or `missing_files`.
    pub status: String,
    /// `trusted`, `unknown` or `revoked`: how the agent treats the signer now.
    pub key_state: String,
    /// The names of its skills.
    pub skills: Vec<String>,
    /// `in_effect` when the agent runs unprivileged, `not_in_effect` when it runs as root.
    pub privilege_boundary: String,
    /// When the hub saw this.
    pub observed_at: String,
    /// `job` or `refresh`.
    pub observed_by: String,
    /// Whether the package's skills can run: `true` only after a person enabled them and the
    /// agent's re-check of the files passed. Absent when the agent is not connected or is too
    /// old to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_enabled: Option<bool>,
    /// Why the skills are off, in the agent's words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_off_reason: Option<String>,
}

impl HostPackageDto {
    /// Fills the enabled state from what the agent announced for this package and version.
    #[must_use]
    pub fn with_skill_states(
        mut self,
        states: &[domovoi_proto::messages::PackageSkillState],
    ) -> Self {
        let mine: Vec<_> = states
            .iter()
            .filter(|s| s.package == self.name && s.version == self.version)
            .collect();
        if mine.is_empty() {
            return self;
        }
        self.skills_enabled = Some(mine.iter().all(|s| s.enabled));
        self.skills_off_reason = mine.iter().find_map(|s| s.reason.clone());
        self
    }
}

impl From<&HostPackage> for HostPackageDto {
    fn from(row: &HostPackage) -> Self {
        Self {
            name: row.name.to_string(),
            version: row.version.to_string(),
            key_id: row.key_id.to_string(),
            archive_sha256: row.archive_sha256.to_string(),
            installed_at: row.installed_at.to_string(),
            status: status_text(row.status).to_owned(),
            key_state: key_state_text(row.key_state).to_owned(),
            skills: row.skills.clone(),
            privilege_boundary: row.privilege_boundary.as_str().to_owned(),
            observed_at: row.observed_at.to_string(),
            observed_by: match row.observed_by {
                ObservedBy::Job => "job",
                ObservedBy::Refresh => "refresh",
            }
            .to_owned(),
            skills_enabled: None,
            skills_off_reason: None,
        }
    }
}

/// What a host reports installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HostPackagesDto {
    /// The host's id.
    pub host_id: String,
    /// When the hub last saw this inventory; absent when it never has.
    pub observed_at: Option<String>,
    /// The installed packages.
    pub entries: Vec<HostPackageDto>,
    /// Whether the host's owner allows installs, when the hub knows. Absent until a refresh
    /// has told it.
    pub install_allowed: Option<bool>,
}

/// Plan a job: what it would do on each host, without doing anything.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanRequestDto {
    /// `install` or `uninstall`.
    pub kind: String,
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: String,
    /// One to sixteen host ids, each once.
    pub host_ids: Vec<String>,
}

/// Create a job. The plan hash is the `job_plan_hash` of the plan the owner looked at.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateJobRequestDto {
    /// `install` or `uninstall`.
    pub kind: String,
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: String,
    /// One to sixteen host ids, each once.
    pub host_ids: Vec<String>,
    /// The `job_plan_hash` of the plan the owner confirmed.
    pub plan_hash: String,
    /// The owner's password, asked for again.
    pub password: Option<String>,
    /// The package name, typed.
    #[serde(default)]
    pub confirm_text: String,
}

/// Turn the skills of a package stored on one host on or off.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageSkillsRequestDto {
    /// `true` to let the skills run, `false` to stop them.
    pub enabled: bool,
    /// The stored version the owner looked at.
    pub version: String,
    /// The owner's password, asked for again.
    pub password: Option<String>,
    /// The package name, typed.
    #[serde(default)]
    pub confirm_text: String,
}

/// What the agent did with a request to enable or disable a package's skills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PackageSkillsDto {
    /// The package.
    pub name: String,
    /// Its version.
    pub version: String,
    /// Whether the skills can run now.
    pub enabled: bool,
    /// Whether anything changed (false when it already was so).
    pub changed: bool,
    /// The plan hash the request was bound to and audited with.
    pub plan_hash: String,
}

/// Run one host's target again.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryRequestDto {
    /// The host's `plan_hash` from the plan the owner confirmed.
    pub plan_hash: String,
    /// The owner's password, asked for again.
    pub password: Option<String>,
    /// The package name, typed.
    #[serde(default)]
    pub confirm_text: String,
}

/// Which jobs to list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct JobsQueryDto {
    /// Only jobs with a target on this host.
    pub host_id: Option<String>,
    /// Only jobs for this package.
    pub name: Option<String>,
    /// `active` for jobs with an unfinished target, `finished` for the others.
    pub status: Option<String>,
    /// Page size, 1 to 200.
    pub limit: Option<u16>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
}

/// What would happen on one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanTargetDto {
    /// The host's id.
    pub host_id: String,
    /// The host's name.
    pub host_name: String,
    /// What approval of this host's work binds: 64 lowercase hex characters.
    pub plan_hash: String,
    /// The state of the capability on this host, such as `available` or `host_absent`.
    pub availability: String,
    /// The reason code of a refused state, when there is one.
    pub availability_reason: Option<String>,
    /// The sentence that explains the state.
    pub availability_text: String,
    /// What the hub last saw of this package on the host.
    pub installed: Option<HostPackageDto>,
    /// Things the owner should know before approving.
    pub warnings: Vec<String>,
}

impl From<&HostPlan> for PlanTargetDto {
    fn from(host: &HostPlan) -> Self {
        Self {
            host_id: host.host.to_string(),
            host_name: host.host_name.to_string(),
            plan_hash: host.plan_hash.to_string(),
            availability: host.availability.code().to_owned(),
            availability_reason: host.availability.reason_code().map(str::to_owned),
            availability_text: host.availability.describe(),
            installed: host.installed.as_ref().map(HostPackageDto::from),
            warnings: host.warnings.clone(),
        }
    }
}

/// What a job would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct InstallPlanDto {
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
    /// The label of the signing key in the hub's owner keys.
    pub signer_label: Option<String>,
    /// What the owner's confirmation binds.
    pub job_plan_hash: String,
    /// One entry per host.
    pub targets: Vec<PlanTargetDto>,
}

impl From<&InstallPlan> for InstallPlanDto {
    fn from(plan: &InstallPlan) -> Self {
        Self {
            kind: kind_text(plan.kind).to_owned(),
            name: plan.name.to_string(),
            version: plan.version.to_string(),
            archive_sha256: plan.archive_sha256.as_ref().map(ToString::to_string),
            key_id: plan.key_id.as_ref().map(ToString::to_string),
            signer_label: plan.signer_label.clone(),
            job_plan_hash: plan.job_plan_hash.to_string(),
            targets: plan.hosts.iter().map(PlanTargetDto::from).collect(),
        }
    }
}

/// The wire text of a job kind.
#[must_use]
pub const fn kind_text(kind: InstallKind) -> &'static str {
    match kind {
        InstallKind::Install => "install",
        InstallKind::Uninstall => "uninstall",
    }
}
