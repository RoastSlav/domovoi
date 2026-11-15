//! Cross-process ownership of the action journal.
//!
//! One exclusive, non-blocking advisory lock over `action-journal.lock` decides which process
//! owns the journal's files. On Linux the guard is `nix`'s owned `Flock<File>`; every other
//! target refuses to run the journal rather than fall back to the in-process mutex.

use std::path::Path;

use crate::error::JournalError;

/// The stable lock file; never renamed, archived or replaced.
pub const LOCK_FILE: &str = "action-journal.lock";

#[cfg(target_os = "linux")]
mod platform {
    use std::os::unix::fs::OpenOptionsExt as _;
    use std::path::Path;

    use nix::errno::Errno;
    use nix::fcntl::{Flock, FlockArg};

    use crate::error::{JournalError, StateError};

    /// Holds the journal lock until dropped, including when the process dies.
    pub(crate) struct OwnerLock {
        _guard: Flock<std::fs::File>,
    }

    impl OwnerLock {
        pub(super) fn acquire(path: &Path) -> Result<Self, JournalError> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .mode(0o600)
                .open(path)
                .map_err(|err| JournalError::State(StateError::new(path, err)))?;
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(guard) => Ok(Self { _guard: guard }),
                Err((file, errno)) => {
                    drop(file);
                    if errno == Errno::EWOULDBLOCK || errno == Errno::EAGAIN {
                        Err(JournalError::Locked(path.to_path_buf()))
                    } else {
                        Err(JournalError::Lock(path.to_path_buf(), errno.to_string()))
                    }
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use std::path::Path;

    use crate::error::JournalError;

    /// A placeholder that always refuses: this platform has no supported journal lock.
    pub(crate) struct OwnerLock;

    impl OwnerLock {
        pub(super) fn acquire(_path: &Path) -> Result<Self, JournalError> {
            Err(JournalError::UnsupportedLock)
        }
    }
}

/// The guard a journal owner holds for its whole lifetime.
pub(super) enum JournalLock {
    /// The real cross-process lock, kept alive by the journal owner.
    Owner(platform::OwnerLock),
    /// A test-only guard that owns nothing.
    None,
}

/// Takes the journal lock on `path`, or reports who owns it.
pub(super) fn acquire(path: &Path) -> Result<JournalLock, JournalError> {
    platform::OwnerLock::acquire(path).map(JournalLock::Owner)
}

/// A test-only lock that owns nothing, so journal logic can run where `Flock` is unavailable.
pub(super) fn unlocked() -> JournalLock {
    JournalLock::None
}
