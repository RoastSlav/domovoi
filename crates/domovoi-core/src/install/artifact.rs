//! An imported package archive as the hub's index knows it.

use jiff::Timestamp;

use crate::actor::Actor;
use crate::id::PackageArtifactId;
use crate::package::{ArchiveDigest, KeyId, PackageName, PackageVersion};

/// How an archive reached the hub's store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactSource {
    /// The owner imported it through the API or the command line.
    OwnerImport,
}

impl ArtifactSource {
    /// The stored text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OwnerImport => "owner_import",
        }
    }

    /// Reads [`ArtifactSource::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "owner_import" => Some(Self::OwnerImport),
            _ => None,
        }
    }
}

/// One imported archive and its detached signature file.
///
/// The hub never opens the archive; this row records which bytes it holds, who vouched for
/// them and who imported them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageArtifact {
    /// Unique id of this import.
    pub id: PackageArtifactId,
    /// The package name the signature covers.
    pub name: PackageName,
    /// The package version the signature covers.
    pub version: PackageVersion,
    /// SHA-256 of the archive's exact bytes.
    pub archive_sha256: ArchiveDigest,
    /// The key that signed it.
    pub key_id: KeyId,
    /// Size of the archive in bytes.
    pub archive_size: u64,
    /// The signature file's text, relayed to hosts unchanged.
    pub signature_toml: String,
    /// The owner's label for the signing key, when the hub's key list has one.
    pub signer_label: Option<String>,
    /// How the archive reached the store.
    pub source: ArtifactSource,
    /// Who imported it.
    pub imported_by: Actor,
    /// When it was imported.
    pub imported_at: Timestamp,
}

/// What a delete of an artifact expects to find: the digest the caller read for that id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactGuard {
    /// The archive digest the stored row must still have.
    pub archive_sha256: ArchiveDigest,
}
