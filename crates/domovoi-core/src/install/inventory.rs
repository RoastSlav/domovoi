//! What the hub last saw installed on a host.

use jiff::Timestamp;

use crate::id::HostId;
use crate::package::{
    ArchiveDigest, KeyId, KeyState, PackageListEntry, PackageName, PackageStatus, PackageVersion,
    PrivilegeBoundary,
};

/// Where an inventory row came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservedBy {
    /// The result of an install job.
    Job,
    /// A refresh that listed the host's packages.
    Refresh,
}

impl ObservedBy {
    /// The stored text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Job => "job",
            Self::Refresh => "refresh",
        }
    }

    /// Reads [`ObservedBy::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "job" => Some(Self::Job),
            "refresh" => Some(Self::Refresh),
            _ => None,
        }
    }
}

/// One package installed on one host, as last observed. The agent is the authority; this is
/// what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPackage {
    /// The host.
    pub host: HostId,
    /// The package name.
    pub name: PackageName,
    /// The installed version.
    pub version: PackageVersion,
    /// The key that signed it.
    pub key_id: KeyId,
    /// Digest of the installed archive.
    pub archive_sha256: ArchiveDigest,
    /// When the agent installed it.
    pub installed_at: Timestamp,
    /// File and signer status.
    pub status: PackageStatus,
    /// The agent's current trust in the signer.
    pub key_state: KeyState,
    /// Names of its skills.
    pub skills: Vec<String>,
    /// Whether the agent's privilege separation was active.
    pub privilege_boundary: PrivilegeBoundary,
    /// When the hub observed this.
    pub observed_at: Timestamp,
    /// What produced the observation.
    pub observed_by: ObservedBy,
}

impl HostPackage {
    /// The row for one `agent.package.list` entry seen on `host`.
    #[must_use]
    pub fn from_entry(
        host: HostId,
        entry: &PackageListEntry,
        privilege_boundary: PrivilegeBoundary,
        observed_at: Timestamp,
        observed_by: ObservedBy,
    ) -> Self {
        Self {
            host,
            name: entry.name.clone(),
            version: entry.version.clone(),
            key_id: entry.key_id.clone(),
            archive_sha256: entry.archive_sha256,
            installed_at: entry.installed_at,
            status: entry.status,
            key_state: entry.key_state,
            skills: entry.skills.clone(),
            privilege_boundary,
            observed_at,
            observed_by,
        }
    }
}
