//! An in-memory package file store for tests.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::install::MAX_CHUNK_BYTES;
use domovoi_core::package::ArchiveDigest;
use domovoi_core::ports::{ArtifactReader, FileError, PackageFiles};
use sha2::{Digest, Sha256};

/// A [`PackageFiles`] that holds archives in memory.
///
/// A test stores bytes under their true digest with [`FakePackageFiles::store`], under a
/// digest they do not match with [`FakePackageFiles::store_as`] (a corrupt file), or leaves
/// the digest out (a missing file).
#[derive(Default)]
pub struct FakePackageFiles {
    files: Mutex<BTreeMap<[u8; 32], Vec<u8>>>,
    opened: Mutex<Vec<ArchiveDigest>>,
}

impl FakePackageFiles {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `bytes` under their SHA-256 and returns it.
    pub fn store(&self, bytes: &[u8]) -> ArchiveDigest {
        let digest = ArchiveDigest::from_bytes(Sha256::digest(bytes).into());
        self.store_as(&digest, bytes);
        digest
    }

    /// Stores `bytes` under `digest`, whether or not they hash to it.
    pub fn store_as(&self, digest: &ArchiveDigest, bytes: &[u8]) {
        crate::lock(&self.files).insert(*digest.as_bytes(), bytes.to_vec());
    }

    /// Removes the file stored under `digest`.
    pub fn remove(&self, digest: &ArchiveDigest) {
        crate::lock(&self.files).remove(digest.as_bytes());
    }

    /// The digests passed to `open`, in order.
    #[must_use]
    pub fn opened(&self) -> Vec<ArchiveDigest> {
        crate::lock(&self.opened).clone()
    }
}

struct MemoryReader {
    bytes: Vec<u8>,
    at: usize,
}

#[async_trait]
impl ArtifactReader for MemoryReader {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FileError> {
        if self.at >= self.bytes.len() {
            return Ok(None);
        }
        let end = (self.at + MAX_CHUNK_BYTES).min(self.bytes.len());
        let chunk = self.bytes[self.at..end].to_vec();
        self.at = end;
        Ok(Some(chunk))
    }
}

#[async_trait]
impl PackageFiles for FakePackageFiles {
    async fn open(&self, digest: &ArchiveDigest) -> Result<Box<dyn ArtifactReader>, FileError> {
        crate::lock(&self.opened).push(*digest);
        let bytes = crate::lock(&self.files)
            .get(digest.as_bytes())
            .cloned()
            .ok_or(FileError::NotFound)?;
        Ok(Box::new(MemoryReader { bytes, at: 0 }))
    }
}
