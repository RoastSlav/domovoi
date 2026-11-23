//! The read-only window onto the host that the helper, the self-test and the probe share.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::identity::{FactsError, GroupEntry, PathFacts, UserEntry};

/// What a probe attempts. Adapters perform exactly the named operation and nothing more.
///
/// `Create`, `Rename`, `Unlink` and `Append` exist only for the isolation probe running inside the
/// runner's read-only view; an adapter that sees one succeed removes what it made and reports
/// [`Access::Allowed`]. No other caller passes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Probe {
    /// `stat` the path.
    Stat,
    /// Open the path for reading.
    Open,
    /// List the directory.
    List,
    /// Connect to the socket.
    Connect,
    /// Create a file in the directory.
    Create,
    /// Rename an entry of the directory.
    Rename,
    /// Remove an entry of the directory.
    Unlink,
    /// Append to an existing file.
    Append,
}

/// The outcome of a [`Probe`], collapsed to what the isolation rules distinguish.
///
/// `NotFound` (`ENOENT`) and `Denied` (`EACCES`) are different answers: a mask hides a path, and a
/// permission error on a path that should be hidden means the mask failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// The operation succeeded.
    Allowed,
    /// `ENOENT`.
    NotFound,
    /// `EACCES` or `EPERM`.
    Denied,
    /// `EROFS`.
    ReadOnly,
    /// Any other error.
    Other,
}

/// One entry of a tree walk, with its facts taken without following symlinks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    /// The entry's full path.
    pub path: PathBuf,
    /// Its type, mode and owner.
    pub facts: PathFacts,
}

/// Why an inspection could not be answered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InspectError {
    /// The path or entry does not exist.
    #[error("not found")]
    NotFound,
    /// The caller may not look.
    #[error("permission denied")]
    PermissionDenied,
    /// Any other failure, with the system's words.
    #[error("{0}")]
    Io(String),
}

impl From<InspectError> for FactsError {
    fn from(err: InspectError) -> Self {
        match err {
            InspectError::NotFound => Self::NotFound,
            InspectError::PermissionDenied => Self::PermissionDenied,
            InspectError::Io(text) => Self::Io(text),
        }
    }
}

/// Reads the host's accounts, file metadata and kernel facts.
///
/// There is no method that changes anything, so a caller holding only this port cannot repair
/// a mode or an owner: that is what makes the helper's start check read-only by construction.
/// Implementations never panic.
pub trait HostInspect: Send + Sync {
    /// The effective uid of this process.
    fn euid(&self) -> u32;
    /// The effective gid of this process.
    fn egid(&self) -> u32;
    /// `getgroups()`: the supplementary groups of this process.
    fn supplementary_groups(&self) -> Result<Vec<u32>, InspectError>;
    /// The account named `name`, or `None` when it does not exist.
    fn user(&self, name: &str) -> Result<Option<UserEntry>, InspectError>;
    /// The group named `name`, or `None` when it does not exist.
    fn group(&self, name: &str) -> Result<Option<GroupEntry>, InspectError>;
    /// Every group id of `user` as `getgrouplist` answers, so NSS-provided groups count.
    fn groups_of(&self, user: &UserEntry) -> Result<Vec<u32>, InspectError>;
    /// Type, mode and owner of `path` without following a symlink.
    fn lstat(&self, path: &Path) -> Result<PathFacts, InspectError>;
    /// Facts of a descriptor opened on `path` with `O_NOFOLLOW`, so an unopenable file is an error.
    fn open_facts(&self, path: &Path) -> Result<PathFacts, InspectError>;
    /// The names in the directory at `path`.
    fn list_dir(&self, path: &Path) -> Result<Vec<String>, InspectError>;
    /// Every entry under `path`, without following symlinks, for the write-bit rule.
    fn walk_tree(&self, path: &Path) -> Result<Vec<TreeEntry>, InspectError>;
    /// Attempts `how` on `path` and reports the result.
    fn probe(&self, path: &Path, how: Probe) -> Access;
    /// The text of a small file such as `/proc/self/status` or `/etc/os-release`.
    fn read_text(&self, path: &Path) -> Result<String, InspectError>;
    /// The running kernel's release string.
    fn kernel_release(&self) -> Result<String, InspectError>;
    /// ST-20: microseconds per cycle of write, fsync, rename and directory fsync in `dir`.
    fn state_write_timings(&self, dir: &Path, cycles: u32) -> Result<Vec<u64>, InspectError>;
}
