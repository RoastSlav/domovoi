//! `[packages]`: the owner's local gate and trust list for installing packages.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::duration::Duration;

/// `[packages]`. Absent means this agent installs nothing and advertises nothing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackagesSection {
    /// Whether the hub may ask this agent to install or remove packages. Default false;
    /// listing installed packages is allowed either way.
    #[serde(default)]
    pub allow_install: bool,
    /// Key ids (16 lowercase hex characters) that are never trusted, even when also listed
    /// in `trusted_keys`.
    #[serde(default)]
    pub revoked: Vec<String>,
    /// Keys whose signatures this agent accepts. Read at start; the hub cannot change them.
    #[serde(default)]
    pub trusted_keys: Vec<TrustedKey>,
    /// Size and time caps applied while receiving and unpacking a package.
    #[serde(default)]
    pub limits: PackageLimits,
}

/// One `[[packages.trusted_keys]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrustedKey {
    /// The key id: the first 16 hex characters of the SHA-256 of the raw public key.
    pub id: String,
    /// The Ed25519 public key, 32 bytes in standard padded base64.
    pub public_key: String,
    /// A note for people, at most 64 characters.
    #[serde(default)]
    pub label: Option<String>,
}

/// `[packages.limits]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct PackageLimits {
    /// Largest archive received, in bytes. 1 KiB to 256 MiB; default 20 MiB.
    pub compressed_bytes: u64,
    /// Most bytes of file content in an archive. 1 KiB to 1 GiB; default 100 MiB.
    pub uncompressed_bytes: u64,
    /// Largest single file, in bytes. At least 1, at most `uncompressed_bytes`; default 20 MiB.
    pub file_bytes: u64,
    /// Most entries in an archive. 1 to 20000; default 2000.
    pub entries: u32,
    /// Deepest path, counting the package directory. 2 to 32; default 8.
    pub path_depth: u32,
    /// Longest path, in bytes. 16 to 1024; default 255.
    pub path_bytes: u32,
    /// Longest time unpacking may take. 1 second to 15 minutes; default 60 seconds.
    pub extraction_time: Duration,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            compressed_bytes: 20 * 1024 * 1024,
            uncompressed_bytes: 100 * 1024 * 1024,
            file_bytes: 20 * 1024 * 1024,
            entries: 2000,
            path_depth: 8,
            path_bytes: 255,
            extraction_time: Duration::secs(60),
        }
    }
}
