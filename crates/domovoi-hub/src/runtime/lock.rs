//! One hub per data directory: an exclusive lock on `<data_dir>/hub.lock`.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// The lock file's name inside the data directory.
pub const LOCK_FILE: &str = "hub.lock";

/// Why the data directory could not be locked.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another process holds the lock: a hub is running on this data directory.
    #[error("a hub is already running on {0}")]
    Held(PathBuf),
    /// The lock file could not be created or locked.
    #[error("could not lock {path}: {source}")]
    Io {
        /// The lock file.
        path: PathBuf,
        /// The operating-system error.
        source: std::io::Error,
    },
}

/// Holds the data directory exclusively until dropped. The operating system releases the
/// lock when the process exits, so a crashed hub never leaves it stuck.
#[derive(Debug)]
pub struct HubLock {
    _file: File,
}

impl HubLock {
    /// Locks `data_dir`, creating it and the lock file when missing. Fails at once with
    /// [`LockError::Held`] when another hub or admin command holds it.
    pub fn acquire(data_dir: &Path) -> Result<Self, LockError> {
        let path = data_dir.join(LOCK_FILE);
        let io = |source| LockError::Io {
            path: path.clone(),
            source,
        };
        std::fs::create_dir_all(data_dir).map_err(io)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::Held(data_dir.to_path_buf())),
            Err(TryLockError::Error(source)) => Err(io(source)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HubLock, LockError};

    #[test]
    fn second_lock_on_the_same_dir_is_refused_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let first = HubLock::acquire(dir.path()).unwrap();
        assert!(matches!(
            HubLock::acquire(dir.path()),
            Err(LockError::Held(_))
        ));
        drop(first);
        HubLock::acquire(dir.path()).unwrap();
    }
}
