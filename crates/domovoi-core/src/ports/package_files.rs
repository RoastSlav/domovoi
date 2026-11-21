//! Reading the bytes of an imported package archive from the hub's content-addressed store.
//!
//! The engine does no I/O, so the install worker takes the archive through this port. The
//! hub implements it over its package directory; the testkit holds files in memory. The
//! reader only hands out bytes: nothing behind this port opens, unpacks or parses an archive.

use async_trait::async_trait;

use crate::package::ArchiveDigest;

/// Why an archive could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileError {
    /// No file is stored for that digest.
    #[error("archive not found")]
    NotFound,
    /// Any other read failure.
    #[error("i/o error: {0}")]
    Io(String),
}

/// A stream over one stored archive, read front to back once.
///
/// Every chunk except the last holds exactly [`MAX_CHUNK_BYTES`](crate::install::MAX_CHUNK_BYTES) bytes and the last holds
/// between 1 and [`MAX_CHUNK_BYTES`](crate::install::MAX_CHUNK_BYTES); an empty file yields no chunk at all.
#[async_trait]
pub trait ArtifactReader: Send {
    /// The next chunk, or `None` after the last one.
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FileError>;
}

/// The archives the hub holds, addressed by the SHA-256 of their bytes.
#[async_trait]
pub trait PackageFiles: Send + Sync {
    /// Opens the stored archive whose digest is `digest` for one front-to-back read.
    ///
    /// The file is not verified against the digest here; the caller hashes what it reads.
    async fn open(&self, digest: &ArchiveDigest) -> Result<Box<dyn ArtifactReader>, FileError>;
}
