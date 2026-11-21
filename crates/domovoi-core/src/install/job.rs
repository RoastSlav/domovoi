//! An install or uninstall job: one request by the owner, one target per host.

use jiff::Timestamp;

use crate::action::PlanHash;
use crate::id::{InstallJobId, UserId};
use crate::package::{ArchiveDigest, KeyId, PackageName, PackageVersion};

/// What a job does on each host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstallKind {
    /// Put a signed package on the host, without enabling it.
    Install,
    /// Remove an installed package from the host.
    Uninstall,
}

impl InstallKind {
    /// The stored text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }

    /// Reads [`InstallKind::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "install" => Some(Self::Install),
            "uninstall" => Some(Self::Uninstall),
            _ => None,
        }
    }
}

/// One owner request to install or remove a package on one or more hosts.
///
/// The per-host state lives in the job's targets; the job itself never changes after it is
/// created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallJob {
    /// Unique id.
    pub id: InstallJobId,
    /// Install or uninstall.
    pub kind: InstallKind,
    /// The package name.
    pub name: PackageName,
    /// The package version.
    pub version: PackageVersion,
    /// The archive digest; present for an install, absent for an uninstall.
    pub archive_sha256: Option<ArchiveDigest>,
    /// The signing key; present for an install, absent for an uninstall.
    pub key_id: Option<KeyId>,
    /// The owner who requested it.
    pub requested_by: UserId,
    /// When it was requested.
    pub created_at: Timestamp,
    /// The hash over every host's plan that the owner saw and confirmed.
    pub job_plan_hash: PlanHash,
}
