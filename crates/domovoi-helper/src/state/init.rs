//! `helper init`: creating the state directory and its lock with the table's modes.

use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

use domovoi_core::identity::{
    FactsError, HostDirs, PathFault, PathId, PathSpec, Principal, evaluate_path, expected_paths,
};
use nix::fcntl::OFlag;

use super::lock::LOCK_FILE;
use crate::inspect::facts_from_metadata;

/// The owner the state directory must have: root in production, the current user in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateLayout {
    /// The uid that owns the directory and the lock.
    pub root_uid: u32,
    /// The gid that owns the directory and the lock.
    pub root_gid: u32,
}

impl StateLayout {
    /// Root, as on an installed host.
    pub const PRODUCTION: Self = Self {
        root_uid: 0,
        root_gid: 0,
    };
}

/// What `init` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitOutcome {
    /// The directory or the lock was created.
    Created,
    /// Both existed with the expected facts; nothing was done.
    AlreadyInitialised,
}

/// Why `init` stopped.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// What exists is not what the table says; nothing was changed. Run `helper repair`.
    #[error("the state directory exists with the wrong facts; nothing was changed")]
    Faults(Vec<PathFault>),
    /// A system call failed.
    #[error("{path}: {source}")]
    Io {
        /// The path being created or looked at.
        path: std::path::PathBuf,
        /// What the system said.
        source: std::io::Error,
    },
}

fn specs(dir: &Path) -> Vec<PathSpec> {
    let dirs = HostDirs {
        helper_dir: dir.to_path_buf(),
        ..HostDirs::default()
    };
    expected_paths(&dirs)
        .into_iter()
        .filter(|spec| matches!(spec.id, PathId::HelperDir | PathId::HelperLock))
        // Resolving both principals against the layout lets a test run as an ordinary user.
        .map(|mut spec| {
            spec.owner = Principal::Agent;
            spec.group = Principal::Agent;
            spec
        })
        .collect()
}

fn look(path: &Path) -> Result<domovoi_core::identity::PathFacts, FactsError> {
    use std::fs::OpenOptions;
    let flags = (OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits();
    match OpenOptions::new().read(true).custom_flags(flags).open(path) {
        Ok(file) => file
            .metadata()
            .map(|m| facts_from_metadata(&m))
            .map_err(|e| FactsError::Io(e.to_string())),
        // A symlink cannot be opened with `O_NOFOLLOW`; report its own facts.
        Err(e) if e.raw_os_error() == Some(nix::errno::Errno::ELOOP as i32) => {
            std::fs::symlink_metadata(path)
                .map(|m| facts_from_metadata(&m))
                .map_err(|e| FactsError::Io(e.to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(FactsError::NotFound),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(FactsError::PermissionDenied)
        }
        Err(e) => Err(FactsError::Io(e.to_string())),
    }
}

fn faults_of(dir: &Path, layout: StateLayout) -> Vec<PathFault> {
    let agent = (layout.root_uid, layout.root_gid);
    specs(dir)
        .iter()
        .filter_map(|spec| evaluate_path(spec, look(&spec.path), agent))
        .collect()
}

/// Creates `dir` (mode 0700) and `dir/state.lock` (mode 0600) when they are absent.
///
/// Anything that already exists must have exactly the expected facts, in which case it is left
/// alone; otherwise the faults are returned and nothing is changed. An existing 0755 directory is
/// never accepted and never corrected here. No key and no state file is created.
pub fn init(layout: StateLayout, dir: &Path) -> Result<InitOutcome, InitError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| InitError::Io { path, source }
    };
    let lock_path = dir.join(LOCK_FILE);
    let mut created = false;
    match std::fs::symlink_metadata(dir) {
        Ok(_) => {
            let faults = faults_of(dir, layout);
            let wrong: Vec<PathFault> = faults
                .into_iter()
                .filter(|f| f.found.is_some() || f.id == PathId::HelperDir)
                .collect();
            if !wrong.is_empty() {
                return Err(InitError::Faults(wrong));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(dir)
                .map_err(io(dir))?;
            created = true;
        }
        Err(e) => return Err(io(dir)(e)),
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(OFlag::O_NOFOLLOW.bits())
        .open(&lock_path)
    {
        Ok(_) => created = true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io(&lock_path)(e)),
    }
    let faults = faults_of(dir, layout);
    if !faults.is_empty() {
        return Err(InitError::Faults(faults));
    }
    Ok(if created {
        InitOutcome::Created
    } else {
        InitOutcome::AlreadyInitialised
    })
}
