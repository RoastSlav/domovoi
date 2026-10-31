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

/// Reads `path`, or `None` when it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, StateError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StateError::new(path, e)),
    }
}
