//! The production [`HostFs`]: host files under a root directory.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use domovoi_core::ports::{FsError, HostFs};
use tokio::io::AsyncReadExt;

/// Largest file [`RealHostFs::read_to_string`] reads.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Reads host files under `root`: `/` normally, `/host` when the agent runs in a container
/// with the host's root mounted there.
///
/// Paths are host-absolute (`/proc/meminfo` reads `<root>/proc/meminfo`). A path that is
/// not absolute or has a `..` component is [`FsError::InvalidPath`]; a file larger than
/// [`MAX_FILE_BYTES`] is refused with [`FsError::Io`].
#[derive(Debug, Clone)]
pub struct RealHostFs {
    root: PathBuf,
}

impl RealHostFs {
    /// A filesystem rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Maps a host-absolute path onto the root.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, FsError> {
        let rest = path.strip_prefix('/').ok_or(FsError::InvalidPath)?;
        let mut resolved = self.root.clone();
        for component in Path::new(rest).components() {
            match component {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                _ => return Err(FsError::InvalidPath),
            }
        }
        Ok(resolved)
    }
}

fn fs_error(error: &std::io::Error) -> FsError {
    match error.kind() {
        ErrorKind::NotFound => FsError::NotFound,
        ErrorKind::PermissionDenied => FsError::PermissionDenied,
        _ => FsError::Io(error.to_string()),
    }
}

#[async_trait]
impl HostFs for RealHostFs {
    async fn read_to_string(&self, path: &str) -> Result<String, FsError> {
        let resolved = self.resolve(path)?;
        let file = tokio::fs::File::open(&resolved)
            .await
            .map_err(|e| fs_error(&e))?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| fs_error(&e))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(FsError::Io(format!(
                "{path} is larger than {MAX_FILE_BYTES} bytes"
            )));
        }
        String::from_utf8(bytes).map_err(|_| FsError::Io(format!("{path} is not UTF-8")))
    }

    async fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        let resolved = self.resolve(path)?;
        let mut entries = tokio::fs::read_dir(&resolved)
            .await
            .map_err(|e| fs_error(&e))?;
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| fs_error(&e))? {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    async fn exists(&self, path: &str) -> bool {
        match self.resolve(path) {
            Ok(p) => tokio::fs::try_exists(p).await.unwrap_or(false),
            Err(_) => false,
        }
    }
}
