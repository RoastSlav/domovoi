//! Requests and results of the three package operations, and how each result becomes the
//! `outputs` map of a `capability_result`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use domovoi_core::package::keys::{
    OUT_ALREADY_INSTALLED, OUT_ARCHIVE_SHA256, OUT_COUNT, OUT_ENABLED, OUT_INSTALL_ALLOWED,
    OUT_INSTALLED_AT, OUT_KEY_ID, OUT_PACKAGE, OUT_PACKAGES, OUT_PRIVILEGE_BOUNDARY, OUT_REMOVED,
    OUT_REPLACED_VERSION, OUT_SIGNER_UPDATED, OUT_SKILLS, OUT_VERSION,
};
use domovoi_core::package::{
    ArchiveDigest, KeyId, KeyState, PackageListEntry, PackageName, PackageVersion,
    PrivilegeBoundary,
};
use jiff::Timestamp;
use uuid::Uuid;

/// Arguments of `agent.package.install`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    /// The package to install.
    pub package: PackageName,
    /// The version to install.
    pub version: PackageVersion,
    /// The stored `package_artifact` transfer holding the archive.
    pub artifact_transfer: Uuid,
    /// The stored `package_manifest` transfer holding the signature file.
    pub signature_transfer: Uuid,
}

/// Arguments of `agent.package.uninstall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallRequest {
    /// The package to remove.
    pub package: PackageName,
    /// The version that must be the installed one.
    pub version: PackageVersion,
}

/// `"true"` or `"false"`, the way outputs carry booleans.
fn flag(value: bool) -> String {
    value.to_string()
}

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// The package.
    pub package: PackageName,
    /// The installed version.
    pub version: PackageVersion,
    /// The signing key.
    pub key_id: KeyId,
    /// The archive digest.
    pub archive_sha256: ArchiveDigest,
    /// When it was (first) installed.
    pub installed_at: Timestamp,
    /// Its skills, sorted.
    pub skills: Vec<String>,
    /// The version this install replaced.
    pub replaced_version: Option<PackageVersion>,
    /// The same archive was already installed; nothing was extracted.
    pub already_installed: bool,
    /// The registered signer was changed to the one that signed this archive.
    pub signer_updated: bool,
    /// Whether the privilege boundary is in effect.
    pub privilege_boundary: PrivilegeBoundary,
}

impl InstallOutcome {
    /// The `outputs` map of the capability result. `enabled` is always `false`.
    #[must_use]
    pub fn outputs(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (OUT_PACKAGE.to_owned(), self.package.to_string()),
            (OUT_VERSION.to_owned(), self.version.to_string()),
            (OUT_KEY_ID.to_owned(), self.key_id.to_string()),
            (
                OUT_ARCHIVE_SHA256.to_owned(),
                self.archive_sha256.to_string(),
            ),
            (OUT_INSTALLED_AT.to_owned(), self.installed_at.to_string()),
            (OUT_SKILLS.to_owned(), self.skills.join(",")),
            (
                OUT_REPLACED_VERSION.to_owned(),
                self.replaced_version
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            ),
            (
                OUT_ALREADY_INSTALLED.to_owned(),
                flag(self.already_installed),
            ),
            (OUT_SIGNER_UPDATED.to_owned(), flag(self.signer_updated)),
            (OUT_ENABLED.to_owned(), flag(false)),
            (
                OUT_PRIVILEGE_BOUNDARY.to_owned(),
                self.privilege_boundary.as_str().to_owned(),
            ),
        ])
    }
}

/// What an uninstall did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallOutcome {
    /// The package asked for.
    pub package: PackageName,
    /// The version asked for.
    pub version: PackageVersion,
    /// Whether a package was actually removed.
    pub removed: bool,
    /// Whether the privilege boundary is in effect.
    pub privilege_boundary: PrivilegeBoundary,
}

impl UninstallOutcome {
    /// The `outputs` map of the capability result.
    #[must_use]
    pub fn outputs(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (OUT_PACKAGE.to_owned(), self.package.to_string()),
            (OUT_VERSION.to_owned(), self.version.to_string()),
            (OUT_REMOVED.to_owned(), flag(self.removed)),
            (
                OUT_PRIVILEGE_BOUNDARY.to_owned(),
                self.privilege_boundary.as_str().to_owned(),
            ),
        ])
    }
}

/// What `agent.package.list` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListOutcome {
    /// Installed packages, sorted by name.
    pub packages: Vec<PackageListEntry>,
    /// Whether this agent allows installs.
    pub install_allowed: bool,
    /// Whether the privilege boundary is in effect.
    pub privilege_boundary: PrivilegeBoundary,
}

impl ListOutcome {
    /// The `outputs` map of the capability result; `packages` is compact JSON.
    #[must_use]
    pub fn outputs(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            (OUT_COUNT.to_owned(), self.packages.len().to_string()),
            (OUT_INSTALL_ALLOWED.to_owned(), flag(self.install_allowed)),
            (
                OUT_PRIVILEGE_BOUNDARY.to_owned(),
                self.privilege_boundary.as_str().to_owned(),
            ),
            (
                OUT_PACKAGES.to_owned(),
                serde_json::to_string(&self.packages).unwrap_or_else(|_| "[]".to_owned()),
            ),
        ])
    }
}

/// An installed package cleared for execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackage {
    /// The package.
    pub name: PackageName,
    /// Its installed version.
    pub version: PackageVersion,
    /// `<packages_dir>/<name>/<version>`.
    pub path: PathBuf,
    /// The key that signed it.
    pub key_id: KeyId,
    /// Its skills.
    pub skills: Vec<String>,
}

/// Why a package may not execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionDenied {
    /// Not registered (or the service is fenced).
    NotInstalled,
    /// A different version is installed.
    VersionMismatch {
        /// The version that is installed.
        installed: PackageVersion,
    },
    /// The signer is not trusted now.
    UntrustedKey {
        /// The signer.
        key_id: KeyId,
        /// Why it is not trusted.
        state: KeyState,
    },
    /// The installed files are gone.
    FilesMissing,
}
