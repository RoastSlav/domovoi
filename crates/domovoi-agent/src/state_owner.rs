//! Handing the agent's state directory to the unprivileged agent account.
//!
//! `domovoi agent enroll` and `agent journal init|reset` run as root and would leave
//! root-owned files that the agent, running as `domovoi-agent`, cannot read. After their last
//! write they call [`hand_over`], which changes owners only: never a mode, never through a
//! symlink.

use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

/// The uid and gid that should own the agent's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateOwner {
    /// The numeric user id.
    pub uid: u32,
    /// The numeric group id.
    pub gid: u32,
}

/// What a hand-over did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandOverReport {
    /// Entries looked at, the directory itself included.
    pub visited: usize,
    /// Entries whose owner or group was changed.
    pub changed: usize,
}

/// Why a hand-over stopped.
#[derive(Debug, thiserror::Error)]
pub enum StateOwnerError {
    /// A symlink was found in the tree; nothing was changed.
    #[error("{} is a symlink; the state directory must not contain one", .0.display())]
    Symlink(PathBuf),
    /// The directory to hand over is not a directory.
    #[error("{} is not a directory", .0.display())]
    NotADirectory(PathBuf),
    /// Reading the tree or changing an owner failed.
    #[error("{}: {reason}", path.display())]
    Io {
        /// The entry.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// The account database could not be read.
    #[error("cannot look up the agent account: {0}")]
    Lookup(String),
    /// This platform cannot change file owners.
    #[error("changing file owners is not supported on this platform")]
    Unsupported,
}

impl StateOwnerError {
    fn io(path: &Path, err: impl std::fmt::Display) -> Self {
        Self::Io {
            path: path.to_path_buf(),
            reason: err.to_string(),
        }
    }
}

/// The agent account's ids, but only when this process is root and the account exists:
/// that is the one case where a hand-over is needed and possible.
pub fn resolve_agent_owner() -> Result<Option<StateOwner>, StateOwnerError> {
    #[cfg(target_os = "linux")]
    {
        if !nix::unistd::geteuid().is_root() {
            return Ok(None);
        }
        let user = nix::unistd::User::from_name(domovoi_core::identity::AGENT_ACCOUNT)
            .map_err(|err| StateOwnerError::Lookup(err.to_string()))?;
        Ok(user.map(|user| StateOwner {
            uid: user.uid.as_raw(),
            gid: user.gid.as_raw(),
        }))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(None)
    }
}

/// Makes `owner` the owner and group of everything under `dir`, `dir` included.
///
/// Walks without following symlinks and refuses the whole tree, changing nothing, when it
/// finds one. Only entries whose owner or group differs are touched, and only their owner:
/// modes stay as they are. Running it twice changes nothing the second time.
pub fn hand_over(dir: &Path, owner: StateOwner) -> Result<HandOverReport, StateOwnerError> {
    hand_over_with(dir, owner, &mut chown_no_follow)
}

/// [`hand_over`] with the ownership change supplied, so a test can run it without being root.
pub fn hand_over_with(
    dir: &Path,
    owner: StateOwner,
    chown: &mut dyn FnMut(&Path, StateOwner) -> io::Result<()>,
) -> Result<HandOverReport, StateOwnerError> {
    let mut entries = Vec::new();
    collect(dir, &mut entries)?;
    let mut report = HandOverReport::default();
    for (path, meta) in entries {
        report.visited += 1;
        let (uid, gid) = ids(&meta).ok_or(StateOwnerError::Unsupported)?;
        if uid == owner.uid && gid == owner.gid {
            continue;
        }
        chown(&path, owner).map_err(|err| StateOwnerError::io(&path, err))?;
        report.changed += 1;
    }
    Ok(report)
}

/// Gathers every entry under `dir`, refusing a symlink anywhere (the root included) before
/// anything is changed.
fn collect(dir: &Path, out: &mut Vec<(PathBuf, Metadata)>) -> Result<(), StateOwnerError> {
    let meta = std::fs::symlink_metadata(dir).map_err(|err| StateOwnerError::io(dir, err))?;
    if meta.file_type().is_symlink() {
        return Err(StateOwnerError::Symlink(dir.to_path_buf()));
    }
    if !meta.is_dir() {
        return Err(StateOwnerError::NotADirectory(dir.to_path_buf()));
    }
    out.push((dir.to_path_buf(), meta));
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let read = std::fs::read_dir(&current).map_err(|err| StateOwnerError::io(&current, err))?;
        for entry in read {
            let entry = entry.map_err(|err| StateOwnerError::io(&current, err))?;
            let path = entry.path();
            let meta =
                std::fs::symlink_metadata(&path).map_err(|err| StateOwnerError::io(&path, err))?;
            if meta.file_type().is_symlink() {
                return Err(StateOwnerError::Symlink(path));
            }
            if meta.is_dir() {
                pending.push(path.clone());
            }
            out.push((path, meta));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn ids(meta: &Metadata) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.uid(), meta.gid()))
}

#[cfg(not(unix))]
fn ids(_meta: &Metadata) -> Option<(u32, u32)> {
    None
}

#[cfg(target_os = "linux")]
fn chown_no_follow(path: &Path, owner: StateOwner) -> io::Result<()> {
    use nix::fcntl::{AT_FDCWD, AtFlags};
    use nix::unistd::{Gid, Uid, fchownat};

    fchownat(
        AT_FDCWD,
        path,
        Some(Uid::from_raw(owner.uid)),
        Some(Gid::from_raw(owner.gid)),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io::Error::from)
}

#[cfg(not(target_os = "linux"))]
fn chown_no_follow(_path: &Path, _owner: StateOwner) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "changing file owners is not supported on this platform",
    ))
}
