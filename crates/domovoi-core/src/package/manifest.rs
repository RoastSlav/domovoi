//! The `package.toml` file at the top of every package archive.

use serde::{Deserialize, Serialize};

use super::{PackageName, PackageVersion};

/// The largest `package.toml` the agent reads, in bytes.
pub const MAX_PACKAGE_MANIFEST_BYTES: usize = 64 * 1024;
/// The most skills one package may carry.
pub const MAX_SKILLS_PER_PACKAGE: usize = 32;

/// What a package says about itself; checked against the signed identity at install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    /// Format number; must be 1.
    pub format: u32,
    /// The package name.
    pub name: PackageName,
    /// The package version.
    pub version: PackageVersion,
    /// Optional description, at most 200 characters.
    #[serde(default)]
    pub description: Option<String>,
    /// What the package needs from the agent.
    pub compat: PackageCompat,
}

/// The agent features a package requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCompat {
    /// Protocol versions the package supports; non-empty.
    pub protocols: Vec<String>,
    /// `<os>-<arch>` platforms; empty or absent means any.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// The skill contract number the package targets.
    pub skill_contract: u32,
}
