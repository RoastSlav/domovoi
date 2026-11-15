//! Making a group of renames survive a crash.
//!
//! A rename changes two directory entries. Until the directory itself is flushed, a crash
//! can leave the old entry, the new entry, or neither. Every group of renames in a restore
//! is therefore bracketed by a [`Fence`]: the moves are made, then `commit` flushes the
//! directory, and a restore that cannot flush it treats that as a failure of the group, not
//! as a warning to ignore.
//!
//! On Unix the flush is `fsync` on the directory. Windows has no portable equivalent, so
//! `commit` there only reports success: the file's own data was flushed before the rename,
//! the renames themselves are ordered, and the preserved previous database is what an
//! operator falls back to if a power loss loses the entry.

use std::path::{Path, PathBuf};

use super::RestoreError;

/// Which group of renames a fence closes, so its failure says what was not made durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// The previous database and its sidecars were moved aside.
    Aside,
    /// The prepared copy was installed as the live database.
    Install,
    /// Files were moved back after a failure.
    Rollback,
}

impl Stage {
    /// The code a test arms in [`SYNC_FAILS`], matching [`Self::describe`].
    fn code(self) -> u8 {
        match self {
            Self::Aside => 1,
            Self::Install => 2,
            Self::Rollback => 3,
        }
    }

    pub(super) fn describe(self) -> &'static str {
        match self {
            Self::Aside => "moving the previous database aside",
            Self::Install => "installing the restored database",
            Self::Rollback => "putting the files back",
        }
    }
}

/// The renames of one stage, and the directory flush that makes them durable.
pub(super) struct Fence {
    stage: Stage,
    database: PathBuf,
    directory: PathBuf,
}

impl Fence {
    /// A fence over the directory holding `database`, for the renames of `stage`.
    pub(super) fn around(database: &Path, stage: Stage) -> Self {
        Self {
            stage,
            database: database.to_path_buf(),
            directory: parent_dir(database),
        }
    }

    /// The database whose directory this fence covers.
    pub(super) fn database(&self) -> &Path {
        &self.database
    }

    /// The directory these renames change.
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    /// What the stage is called in an error.
    pub(super) fn stage_name(&self) -> &'static str {
        self.stage.describe()
    }

    /// Flushes the directory so the renames in this stage survive a crash.
    ///
    /// The error names the stage and the directory: the rename may have happened, but
    /// nothing can promise it was recorded.
    pub(super) fn commit(&self) -> Result<(), RestoreError> {
        sync_dir_for(&self.directory, self.stage).map_err(|err| RestoreError::NotDurable {
            stage: self.stage.describe().to_owned(),
            path: self.directory.clone(),
            reason: err,
        })
    }
}

/// The directory holding `path`, or the current one when it has no parent.
fn parent_dir(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

/// Flushes `dir`.
#[cfg(unix)]
pub(super) fn sync_dir(dir: &Path) -> Result<(), String> {
    let handle = std::fs::File::open(dir).map_err(|err| err.to_string())?;
    handle.sync_all().map_err(|err| err.to_string())
}

/// The same flush, for `stage`, honouring a test's request to fail it.
#[cfg(unix)]
fn sync_dir_for(dir: &Path, stage: Stage) -> Result<(), String> {
    if armed_for(dir, stage) {
        return Err("simulated failure".to_owned());
    }
    sync_dir(dir)
}

/// Windows has no directory flush, but a test can still ask the stage to fail, so the
/// recovery paths are covered on both platforms.
#[cfg(not(unix))]
fn sync_dir_for(dir: &Path, stage: Stage) -> Result<(), String> {
    if armed_for(dir, stage) {
        return Err("simulated failure".to_owned());
    }
    Ok(())
}

/// The flushes a test asked to fail, keyed by directory and stage. A registry rather than
/// one flag, because tests run in parallel and a shared flag would be consumed by whichever
/// restore happened to flush first.
pub(super) static SYNC_FAILS: std::sync::Mutex<Vec<(PathBuf, u8)>> =
    std::sync::Mutex::new(Vec::new());

/// Makes the flush of `stage` for the data directory holding `database` fail once.
#[doc(hidden)]
pub fn fail_next_sync_for_tests(database: &Path, stage: &str) {
    let code = match stage {
        "aside" => 1,
        "install" => 2,
        "rollback" => 3,
        _ => 0,
    };
    if code == 0 {
        return;
    }
    if let Ok(mut armed) = SYNC_FAILS.lock() {
        armed.push((parent_dir(database), code));
    }
}

/// Whether this flush is the one a test asked to fail, consuming the request.
fn armed_for(dir: &Path, stage: Stage) -> bool {
    let Ok(mut armed) = SYNC_FAILS.lock() else {
        return false;
    };
    let code = stage.code();
    let Some(index) = armed
        .iter()
        .position(|(wanted, armed)| wanted == dir && *armed == code)
    else {
        return false;
    };
    armed.remove(index);
    true
}

/// No portable directory fsync on Windows (see the module note).
#[cfg(not(unix))]
pub(super) fn sync_dir(_dir: &Path) -> Result<(), String> {
    Ok(())
}
