//! Read-only access to a host's filesystem.

use async_trait::async_trait;

/// Why a host file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FsError {
    /// Nothing exists at the path.
    #[error("not found")]
    NotFound,
    /// The process may not read the path.
    #[error("permission denied")]
    PermissionDenied,
    /// The path is not absolute or contains a `..` component.
    #[error("invalid path")]
    InvalidPath,
    /// Any other I/O failure, including content that is not UTF-8.
    #[error("i/o error: {0}")]
    Io(String),
}

/// Reads files under a host root such as `/proc`, `/sys` or log directories.
///
/// Paths are host-absolute (`/proc/meminfo`). Adapters reject paths that are not absolute
/// or that contain a `..` component with [`FsError::InvalidPath`] before touching the disk.
#[async_trait]
pub trait HostFs: Send + Sync {
    /// Reads a whole file as UTF-8.
    async fn read_to_string(&self, path: &str) -> Result<String, FsError>;

    /// Lists the entry names (not full paths) of a directory, sorted ascending.
    async fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError>;

    /// Whether anything exists at `path`. Invalid paths are reported as not existing.
    async fn exists(&self, path: &str) -> bool;
}
