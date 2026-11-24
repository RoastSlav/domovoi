//! Crash-safe replacement of a file in the state directory.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use nix::fcntl::OFlag;

use super::lock::LockGuard;

/// Stages of a write that a test can make fail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WriteFaults {
    /// Fail after the temporary file is written and synced, before the rename.
    pub temp_sync: bool,
    /// Fail at the rename.
    pub rename: bool,
    /// Fail the directory sync after a successful rename.
    pub dir_sync: bool,
}

/// A write that did not complete, and how much of it took effect.
#[derive(Debug, thiserror::Error)]
pub enum StateWriteError {
    /// Nothing changed: the previous file, if any, is still the file.
    #[error("the write of {path} did not happen: {source}")]
    Failed {
        /// The file being replaced.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The new file is in place but the directory sync failed, so a power failure might lose it.
    #[error("{path} was replaced but may not survive a power failure: {source}")]
    DurabilityUnknown {
        /// The file that was replaced.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
}

fn injected(what: &str) -> std::io::Error {
    std::io::Error::other(format!("injected {what} failure"))
}

/// Replaces `dir/name` with `bytes` at `mode`: write a temporary file, sync it, rename it over
/// the target and sync the directory.
///
/// A leftover temporary file from an earlier crash is removed first. The temporary file is
/// created exclusively and without following a symlink. `_guard` is proof that the state lock is
/// held, so no other helper instance can be writing at the same time.
pub fn write_atomic_locked(
    _guard: &LockGuard<'_>,
    dir: &Path,
    name: &str,
    bytes: &[u8],
    mode: u32,
    faults: WriteFaults,
) -> Result<(), StateWriteError> {
    let target = dir.join(name);
    let failed = |source| StateWriteError::Failed {
        path: target.clone(),
        source,
    };
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(failed(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the name must be a single path component",
        )));
    }
    let temp = dir.join(format!(".{name}.tmp"));
    match std::fs::remove_file(&temp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed(e)),
    }
    if let Err(e) = stage(&temp, bytes, mode, faults) {
        let _ = std::fs::remove_file(&temp);
        return Err(failed(e));
    }
    let renamed = if faults.rename {
        Err(injected("rename"))
    } else {
        std::fs::rename(&temp, &target)
    };
    if let Err(e) = renamed {
        let _ = std::fs::remove_file(&temp);
        return Err(failed(e));
    }
    let synced = if faults.dir_sync {
        Err(injected("directory sync"))
    } else {
        std::fs::File::open(dir).and_then(|d| d.sync_all())
    };
    synced.map_err(|source| StateWriteError::DurabilityUnknown {
        path: target.clone(),
        source,
    })
}

fn stage(temp: &Path, bytes: &[u8], mode: u32, faults: WriteFaults) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(temp)?;
    // The process umask may have removed bits at creation.
    file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if faults.temp_sync {
        return Err(injected("temporary-file sync"));
    }
    Ok(())
}
