//! Atomic writes for files under the state directory.

use std::io::Write;
use std::path::Path;

use crate::error::StateError;

/// Writes `bytes` to `path` through a temporary file in the same directory, then renames it
/// over `path`, so readers see the old or the new content and never a mix.
///
/// On Unix the file gets `mode` before any content is written.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), StateError> {
    let dir = path
        .parent()
        .ok_or_else(|| StateError::new(path, "has no parent directory"))?;
    std::fs::create_dir_all(dir).map_err(|e| StateError::new(dir, e))?;
    let name = path
        .file_name()
        .ok_or_else(|| StateError::new(path, "has no file name"))?;
    let tmp = dir.join(format!(".{}.tmp", name.to_string_lossy()));
    let mut file = open_new(&tmp, mode).map_err(|e| StateError::new(&tmp, e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| StateError::new(&tmp, e))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|e| StateError::new(path, e))
}

#[cfg(unix)]
fn open_new(path: &Path, mode: u32) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    // A leftover temporary file keeps its old mode, so set it again.
    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    Ok(file)
}

#[cfg(not(unix))]
fn open_new(path: &Path, _mode: u32) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

/// Stages of a durable write a test can make fail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DurableFaults {
    /// Fail after writing and syncing the temporary file, before the rename.
    pub temp_sync: bool,
    /// Fail at the rename.
    pub rename: bool,
    /// Fail the directory `fsync` after a successful rename.
    pub dir_sync: bool,
}

/// A durable write that failed, and what it left behind.
#[derive(Debug, thiserror::Error)]
pub enum DurableWriteError {
    /// The previous file is authoritative; the update was not committed.
    #[error("the write was not committed: {0}")]
    BeforeCommit(StateError),
    /// The rename succeeded but the directory `fsync` failed, so durability is unknown.
    #[error("the write may or may not survive a power failure: {0}")]
    Indeterminate(StateError),
}

impl DurableWriteError {}

/// Like [`write_atomic`], then `fsync` the parent directory so the rename itself is durable.
///
/// On Windows a directory cannot be opened for syncing; the rename then rests on the file
/// sync alone, which is documented rather than silently claimed.
pub fn write_atomic_durable(path: &Path, bytes: &[u8], mode: u32) -> Result<(), DurableWriteError> {
    write_atomic_durable_with(path, bytes, mode, DurableFaults::default())
}

/// [`write_atomic_durable`] with test-injectable failures.
pub fn write_atomic_durable_with(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    faults: DurableFaults,
) -> Result<(), DurableWriteError> {
    let dir = path
        .parent()
        .ok_or_else(|| DurableWriteError::BeforeCommit(StateError::new(path, "has no parent")))?;
    std::fs::create_dir_all(dir)
        .map_err(|e| DurableWriteError::BeforeCommit(StateError::new(dir, e)))?;
    let name = path.file_name().ok_or_else(|| {
        DurableWriteError::BeforeCommit(StateError::new(path, "has no file name"))
    })?;
    let tmp = dir.join(format!(".{}.tmp", name.to_string_lossy()));
    let mut file = open_new(&tmp, mode)
        .map_err(|e| DurableWriteError::BeforeCommit(StateError::new(&tmp, e)))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| DurableWriteError::BeforeCommit(StateError::new(&tmp, e)))?;
    drop(file);
    if faults.temp_sync {
        return Err(DurableWriteError::BeforeCommit(StateError::new(
            &tmp,
            "injected temporary-file sync failure",
        )));
    }
    if faults.rename {
        return Err(DurableWriteError::BeforeCommit(StateError::new(
            path,
            "injected rename failure",
        )));
    }
    std::fs::rename(&tmp, path)
        .map_err(|e| DurableWriteError::BeforeCommit(StateError::new(path, e)))?;
    if faults.dir_sync {
        return Err(DurableWriteError::Indeterminate(StateError::new(
            dir,
            "injected directory sync failure",
        )));
    }
    sync_dir(dir).map_err(|e| DurableWriteError::Indeterminate(StateError::new(dir, e)))
}

/// `fsync`s a directory so a rename inside it is durable.
///
/// On Windows a directory cannot be opened for syncing; the rename rests on the file sync.
pub(crate) fn sync_dir(dir: &Path) -> std::io::Result<()> {
    sys_sync_dir(dir)
}

#[cfg(unix)]
fn sys_sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sys_sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Reads `path`, or `None` when it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StateError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StateError::new(path, e)),
    }
}
