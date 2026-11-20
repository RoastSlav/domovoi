//! Argument and output keys of the `agent.package.*` capabilities, and the list entry type.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::{ArchiveDigest, KeyId, PackageName, PackageVersion};

/// Keys of the capability arguments and outputs. Booleans are `"true"`/`"false"` strings.
pub mod keys {
    /// Argument: the package name.
    pub const ARG_PACKAGE: &str = "package";
    /// Argument: the package version.
    pub const ARG_VERSION: &str = "version";
    /// Argument: id of the transfer that carries the archive.
    pub const ARG_ARTIFACT_TRANSFER: &str = "artifact_transfer";
    /// Argument: id of the transfer that carries the signature file.
    pub const ARG_SIGNATURE_TRANSFER: &str = "signature_transfer";
    /// Output: the package name.
    pub const OUT_PACKAGE: &str = "package";
    /// Output: the package version.
    pub const OUT_VERSION: &str = "version";
    /// Output: the signing key id.
    pub const OUT_KEY_ID: &str = "key_id";
    /// Output: the archive digest.
    pub const OUT_ARCHIVE_SHA256: &str = "archive_sha256";
    /// Output: when the package was installed.
    pub const OUT_INSTALLED_AT: &str = "installed_at";
    /// Output: sorted skill names joined by `,`.
    pub const OUT_SKILLS: &str = "skills";
    /// Output: the version this install replaced, or empty.
    pub const OUT_REPLACED_VERSION: &str = "replaced_version";
    /// Output: whether the same archive was already installed.
    pub const OUT_ALREADY_INSTALLED: &str = "already_installed";
    /// Output: whether the recorded signer changed.
    pub const OUT_SIGNER_UPDATED: &str = "signer_updated";
    /// Output: always `"false"`; install never enables.
    pub const OUT_ENABLED: &str = "enabled";
    /// Output: `in_effect` or `not_in_effect`.
    pub const OUT_PRIVILEGE_BOUNDARY: &str = "privilege_boundary";
    /// Output: whether an uninstall removed anything.
    pub const OUT_REMOVED: &str = "removed";
    /// Output: number of installed packages.
    pub const OUT_COUNT: &str = "count";
    /// Output: whether the owner allows installs on this agent.
    pub const OUT_INSTALL_ALLOWED: &str = "install_allowed";
    /// Output: compact JSON of `Vec<PackageListEntry>`, sorted by name.
    pub const OUT_PACKAGES: &str = "packages";
}

/// Whether an installed package's files and signer are in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageStatus {
    /// Registered, files present, signer trusted.
    Active,
    /// The signer is no longer trusted or was revoked.
    DisabledUntrustedKey,
    /// Registered but its files are gone.
    MissingFiles,
}

/// How the agent's key set treats a package's signer now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    /// In the trusted set and not revoked.
    Trusted,
    /// Not in the trusted set.
    Unknown,
    /// On the revocation list.
    Revoked,
}

/// Whether the agent's privilege separation is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeBoundary {
    /// The agent runs unprivileged.
    InEffect,
    /// The agent runs as root.
    NotInEffect,
}

impl PrivilegeBoundary {
    /// The output text of this value.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::InEffect => "in_effect",
            Self::NotInEffect => "not_in_effect",
        }
    }
}

/// One installed package as `agent.package.list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageListEntry {
    /// The package name.
    pub name: PackageName,
    /// The installed version.
    pub version: PackageVersion,
    /// The key that signed it.
    pub key_id: KeyId,
    /// Digest of the installed archive.
    pub archive_sha256: ArchiveDigest,
    /// When it was installed.
    pub installed_at: Timestamp,
    /// Names of its skills.
    pub skills: Vec<String>,
    /// File and signer status.
    pub status: PackageStatus,
    /// Current trust in the signer.
    pub key_state: KeyState,
    /// Whether its skills can run now.
    pub executable: bool,
    /// Why not, when `executable` is false.
    pub executable_reason: Option<String>,
}
