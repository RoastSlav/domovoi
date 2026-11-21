//! The store as the engine's [`PackageFiles`] port: stored archives read back in link-sized
//! chunks. The bytes are handed on untouched; nothing here opens an archive.

use async_trait::async_trait;
use domovoi_core::install::MAX_CHUNK_BYTES;
use domovoi_core::package::ArchiveDigest;
use domovoi_core::ports::{ArtifactReader, FileError, PackageFiles};
use tokio::fs::File;
use tokio::io::AsyncReadExt as _;

use super::store::PackageStore;

/// One stored archive, read front to back in chunks of [`MAX_CHUNK_BYTES`].
struct FileReader {
    file: File,
}

#[async_trait]
impl ArtifactReader for FileReader {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FileError> {
        let mut chunk = vec![0u8; MAX_CHUNK_BYTES];
        let mut filled = 0;
        while filled < chunk.len() {
            let read = self
                .file
                .read(&mut chunk[filled..])
                .await
                .map_err(|err| FileError::Io(err.to_string()))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        chunk.truncate(filled);
        Ok((filled > 0).then_some(chunk))
    }
}

#[async_trait]
impl PackageFiles for PackageStore {
    async fn open(&self, digest: &ArchiveDigest) -> Result<Box<dyn ArtifactReader>, FileError> {
        match File::open(self.path_of(digest)).await {
            Ok(file) => Ok(Box::new(FileReader { file })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(FileError::NotFound),
            Err(err) => Err(FileError::Io(err.to_string())),
        }
    }
}
