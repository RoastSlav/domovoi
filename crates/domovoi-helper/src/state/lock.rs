//! The advisory lock that serialises helper instances over the state directory.

use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg, OFlag};

/// The lock file's name inside the state directory.
pub const LOCK_FILE: &str = "state.lock";

/// Why the lock could not be opened or taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// The lock file is a symlink; it is never followed.
    #[error("{0} is a symlink")]
    Symlink(PathBuf),
    /// The lock path is not a regular file.
    #[error("{0} is not a regular file")]
    NotARegularFile(PathBuf),
    /// The lock file could not be opened.
    #[error("cannot open {path}: {source}")]
    Open {
        /// The lock file.
        path: PathBuf,
        /// What the system said.
        source: std::io::Error,
    },
    /// `flock` failed for a reason other than contention.
    #[error("cannot lock {path}: {errno}")]
    Lock {
        /// The lock file.
        path: PathBuf,
        /// The system error.
        errno: Errno,
    },
    /// This lock object already has a live guard; taking it again would deadlock.
    #[error("{0} is already held by this process")]
    AlreadyHeld(PathBuf),
}

/// An open lock file that has not necessarily been locked yet.
#[derive(Debug)]
pub struct StateLock {
    file: File,
    path: PathBuf,
    held: AtomicBool,
}

/// Proof that the lock is held; releases it when dropped.
#[derive(Debug)]
pub struct LockGuard<'a> {
    flock: Option<Flock<File>>,
    owner: &'a StateLock,
}

impl StateLock {
    /// Opens `state.lock` in `dir` without following a symlink and checks it is a regular file.
    pub fn open(dir: &Path) -> Result<Self, LockError> {
        let path = dir.join(LOCK_FILE);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(OFlag::O_NOFOLLOW.bits())
            .open(&path)
            .map_err(|source| {
                if source.raw_os_error() == Some(Errno::ELOOP as i32) {
                    LockError::Symlink(path.clone())
                } else {
                    LockError::Open {
                        path: path.clone(),
                        source,
                    }
                }
            })?;
        let metadata = file.metadata().map_err(|source| LockError::Open {
            path: path.clone(),
            source,
        })?;
        if !metadata.is_file() {
            return Err(LockError::NotARegularFile(path));
        }
        Ok(Self {
            file,
            path,
            held: AtomicBool::new(false),
        })
    }

    fn lock(&self, arg: FlockArg) -> Result<Option<LockGuard<'_>>, LockError> {
        if self.held.swap(true, Ordering::AcqRel) {
            return Err(LockError::AlreadyHeld(self.path.clone()));
        }
        let release = |lock: &Self| lock.held.store(false, Ordering::Release);
        loop {
            let file = match self.file.try_clone() {
                Ok(file) => file,
                Err(source) => {
                    release(self);
                    return Err(LockError::Open {
                        path: self.path.clone(),
                        source,
                    });
                }
            };
            match Flock::lock(file, arg) {
                Ok(flock) => {
                    return Ok(Some(LockGuard {
                        flock: Some(flock),
                        owner: self,
                    }));
                }
                Err((_, Errno::EINTR)) => {}
                Err((_, Errno::EWOULDBLOCK)) => {
                    release(self);
                    return Ok(None);
                }
                Err((_, errno)) => {
                    release(self);
                    return Err(LockError::Lock {
                        path: self.path.clone(),
                        errno,
                    });
                }
            }
        }
    }

    /// Takes the exclusive lock without waiting; `None` means another holder has it.
    pub fn try_acquire(&self) -> Result<Option<LockGuard<'_>>, LockError> {
        self.lock(FlockArg::LockExclusiveNonblock)
    }

    /// Takes the exclusive lock, waiting for the current holder to let go.
    pub fn acquire(&self) -> Result<LockGuard<'_>, LockError> {
        self.lock(FlockArg::LockExclusive)?
            .ok_or_else(|| LockError::AlreadyHeld(self.path.clone()))
    }
}

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        // Release the kernel lock first, then let this process take it again.
        drop(self.flock.take());
        self.owner.held.store(false, Ordering::Release);
    }
}
