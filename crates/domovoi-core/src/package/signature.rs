//! The detached signature file and the exact bytes it signs.

use serde::{Deserialize, Serialize};

use super::{ArchiveDigest, KeyId, PackageName, PackageVersion};

/// The only signature file format this version reads.
pub const SIGNATURE_FORMAT: u32 = 1;
/// The only signature algorithm accepted.
pub const SIGNATURE_ALGORITHM: &str = "ed25519";
/// The largest signature file the agent accepts, in bytes.
pub const MAX_SIGNATURE_FILE_BYTES: usize = 16 * 1024;
/// The most packages one agent keeps installed.
pub const MAX_INSTALLED_PACKAGES: usize = 64;

/// The detached `<name>-<version>.sig.toml` that vouches for one archive.
///
/// Parsing checks the shape only; the caller must still check `format` and `algorithm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureFile {
    /// Format number; must equal [`SIGNATURE_FORMAT`].
    pub format: u32,
    /// Signature algorithm; must equal [`SIGNATURE_ALGORITHM`].
    pub algorithm: String,
    /// The key that made the signature.
    pub key_id: KeyId,
    /// The package the signature covers.
    pub package: PackageName,
    /// The version the signature covers.
    pub version: PackageVersion,
    /// SHA-256 of the archive the signature covers.
    pub archive_sha256: ArchiveDigest,
    /// Standard padded base64 of exactly 64 bytes; decoded by the agent, not by core.
    pub signature: String,
}

/// The bytes that are signed:
/// `"domovoi-package-v1\n{package}\n{version}\n{archive_sha256}\n{key_id}\n"`.
#[must_use]
pub fn signing_message(
    package: &PackageName,
    version: &PackageVersion,
    digest: &ArchiveDigest,
    key_id: &KeyId,
) -> Vec<u8> {
    format!("domovoi-package-v1\n{package}\n{version}\n{digest}\n{key_id}\n").into_bytes()
}
