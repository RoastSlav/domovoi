//! The SHA-256 of a package archive.

use crate::sha256::sha256_newtype;

sha256_newtype!(
    /// SHA-256 of a package archive's exact bytes, written as lowercase hex.
    ArchiveDigest
);

impl ArchiveDigest {
    /// Hashes exactly `bytes` with SHA-256.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self::digest(bytes)
    }

    /// Wraps a hash computed elsewhere, for example while streaming the archive.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}
