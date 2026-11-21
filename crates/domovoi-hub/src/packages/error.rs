//! Why an import or a delete of a package was refused.

use domovoi_core::package::{KeyId, PackageName, PackageVersion};
use domovoi_core::ports::{CommitError, StoreError};

/// Why the hub did not import, or did not delete, a package.
///
/// Every refusal has a stable [`code`](ImportError::code) that the API and the command line
/// print unchanged; the message is for people and never lists the owner keys.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// `[packages] enabled` is off.
    #[error("packages are not enabled on this hub")]
    PackagesDisabled,
    /// The signature file is too big, is not TOML of the expected shape, or names a format
    /// or algorithm this hub does not read.
    #[error("the signature file is not acceptable: {0}")]
    SignatureFileInvalid(String),
    /// The archive does not start with the zstd magic number.
    #[error("the archive is not zstd-compressed")]
    NotZstd,
    /// The archive is longer than `max_artifact_bytes`.
    #[error("the archive is larger than the {max} bytes this hub accepts")]
    TooLarge {
        /// The configured limit.
        max: u64,
    },
    /// The signing key is not one of the hub's owner keys.
    #[error("the signing key {0} is not one of this hub's owner keys")]
    UnknownKey(KeyId),
    /// The signing key is on the hub's revocation list.
    #[error("the signing key {0} is revoked")]
    RevokedKey(KeyId),
    /// The signature is not a valid Ed25519 signature of the package, version, digest and key.
    #[error("the signature does not verify")]
    BadSignature,
    /// The archive's SHA-256 is not the digest the signature file covers.
    #[error("the archive does not match the digest in the signature file")]
    DigestMismatch,
    /// A store limit (`max_artifacts` or `max_store_bytes`) would be exceeded.
    #[error("the package store is full: {0}")]
    StoreFull(String),
    /// The package and version are already imported with a different archive.
    #[error("{name}@{version} is already imported with a different archive")]
    PackageConflict {
        /// The package.
        name: PackageName,
        /// Its version.
        version: PackageVersion,
    },
    /// A running install job still uses the archive.
    #[error("the package is used by a running install job")]
    PackageInUse,
    /// There is no such imported package.
    #[error("no such package")]
    NoSuchPackage,
    /// Reading the uploaded bytes failed.
    #[error("reading the upload failed: {0}")]
    Upload(String),
    /// The package files could not be read or written.
    #[error("the package store failed: {0}")]
    Io(#[from] std::io::Error),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The commit failed for a reason that is not a refusal.
    #[error(transparent)]
    Commit(CommitError),
}

impl ImportError {
    /// The stable code: the same text the API's error body and the command line carry.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::PackagesDisabled => "packages_disabled",
            Self::SignatureFileInvalid(_) => "signature_file_invalid",
            Self::NotZstd => "not_zstd",
            Self::TooLarge { .. } => "too_large",
            Self::UnknownKey(_) => "unknown_key",
            Self::RevokedKey(_) => "revoked_key",
            Self::BadSignature => "bad_signature",
            Self::DigestMismatch => "digest_mismatch",
            Self::StoreFull(_) => "store_full",
            Self::PackageConflict { .. } => "package_conflict",
            Self::PackageInUse => "package_in_use",
            Self::NoSuchPackage => "no_such_package",
            Self::Upload(_) => "upload_failed",
            Self::Io(_) | Self::Store(_) | Self::Commit(_) => "internal",
        }
    }

    /// Whether the caller sent something the hub refuses, as opposed to the hub failing.
    /// Only these are worth an `import_rejected` audit row.
    #[must_use]
    pub const fn is_rejection(&self) -> bool {
        matches!(
            self,
            Self::SignatureFileInvalid(_)
                | Self::NotZstd
                | Self::TooLarge { .. }
                | Self::UnknownKey(_)
                | Self::RevokedKey(_)
                | Self::BadSignature
                | Self::DigestMismatch
                | Self::StoreFull(_)
                | Self::PackageConflict { .. }
        )
    }
}
