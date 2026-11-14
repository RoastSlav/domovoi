//! Installing a backup as the live database, keeping the one it replaces.
//!
//! The caller holds the data directory's lock: nothing runs this against a database a
//! hub is using. Nothing here deletes the previous database, and a failure part-way puts
//! it back, so an operator never ends without one.

use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::backup::{BackupError, BackupReport, Depth, verify_backup};
use crate::db::{Store, StoreOptions};

/// What a restore did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    /// The backup that was installed.
    pub backup: BackupReport,
    /// Where the database it replaced was moved; `None` when there was none.
    pub previous: Option<PathBuf>,
    /// Migrations run when the restored file was opened.
    pub migrations_applied_after: usize,
}

/// Why a backup could not be installed.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// The backup is not a database that can be restored.
    #[error(transparent)]
    Backup(#[from] BackupError),
    /// A file could not be moved, copied or removed.
    #[error("{}: {reason}", path.display())]
    Io {
        /// The path involved.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// The restored database could not be opened, migrated or checked.
    #[error(transparent)]
    Open(#[from] crate::error::StoreOpenError),
    /// The backup is the database itself.
    #[error("the backup and the database are the same file")]
    SameFile,
}

fn io_err(path: &Path, err: impl std::fmt::Display) -> RestoreError {
    RestoreError::Io {
        path: path.to_owned(),
        reason: err.to_string(),
    }
}

fn suffix(database: &Path, tail: &str) -> PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push(tail);
    PathBuf::from(name)
}

/// Every path SQLite keeps for `database`: the file, its write-ahead log and its shared
/// memory index.
fn companions(database: &Path) -> [(PathBuf, &'static str); 3] {
    [
        (database.to_path_buf(), ""),
        (suffix(database, "-wal"), "-wal"),
        (suffix(database, "-shm"), "-shm"),
    ]
}

/// The same file, judged by canonical path where both exist.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The migration count of an open store.
async fn applied_count(store: &Store) -> Result<usize, sqlx::Error> {
    sqlx::query_as::<_, (i64,)>("SELECT version FROM _sqlx_migrations WHERE success = 1")
        .fetch_all(store.pool_for_tests())
        .await
        .map(|rows| rows.len())
}

/// Installs `backup` as `database`. The caller holds the data-directory lock.
///
/// The backup is verified in full before anything moves. It is then copied beside the
/// database, made owner-only on Unix, synced, and opened there so pending migrations run;
/// only a copy that opens is swapped in. The database it replaces, with its `-wal` and
/// `-shm`, is renamed to `<database>.before-restore-<time>` first, so a failed restore
/// never touches the live database and never leaves the operator without one.
pub async fn restore_from_backup(
    database: &Path,
    backup: &Path,
    now: Timestamp,
) -> Result<RestoreReport, RestoreError> {
    if same_file(database, backup) {
        return Err(RestoreError::SameFile);
    }
    let report = verify_backup(backup, Depth::Full).await?;
    let name = database
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let stamp = stamp(now);
    let before = database.with_file_name(format!("{name}.before-restore-{stamp}"));
    let temporary = suffix(database, ".restore-tmp");

    // Migrate the copy under its temporary name: a backup that cannot be opened leaves
    // the live database exactly as it was.
    let applied = match prepare(backup, &temporary, report.migrations).await {
        Ok(applied) => applied,
        Err(PrepareError::Copy(err)) => return Err(err),
        Err(PrepareError::Open(err)) => {
            // Keep the copy that would not open, under a name that says so. The failed
            // open may still hold its file on Windows, where the rename then fails and
            // the copy stays at its temporary name; the log names whichever applies.
            let failed = database.with_file_name(format!("{name}.failed-restore-{stamp}"));
            let mut kept = failed.clone();
            for tail in ["", "-wal", "-shm"] {
                if std::fs::rename(suffix(&temporary, tail), suffix(&failed, tail)).is_err() {
                    kept = suffix(&temporary, tail);
                }
            }
            tracing::warn!(
                failed = %kept.display(),
                "the restored database did not open; it is kept for inspection",
            );
            return Err(err);
        }
    };
    let previous = move_aside(database, &before)?;
    if let Err(err) = swap_in(&temporary, database) {
        // The live database is back where it was; the prepared copy is not worth keeping.
        if previous.is_some() {
            for tail in ["", "-wal", "-shm"] {
                let _ = std::fs::rename(suffix(&before, tail), suffix(database, tail));
            }
        }
        for tail in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(suffix(&temporary, tail));
        }
        return Err(err);
    }
    Ok(RestoreReport {
        backup: report,
        previous,
        migrations_applied_after: applied,
    })
}

fn stamp(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// Why preparing the copy failed: while copying it, or while opening it.
enum PrepareError {
    Copy(RestoreError),
    Open(RestoreError),
}

/// Copies `backup` to `temporary`, makes it owner-only, syncs it, and opens it there so
/// pending migrations run. Returns how many ran, against the `installed` the backup
/// already recorded.
async fn prepare(backup: &Path, temporary: &Path, installed: usize) -> Result<usize, PrepareError> {
    let copy = || -> Result<(), RestoreError> {
        std::fs::copy(backup, temporary).map_err(|err| io_err(temporary, err))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(temporary, std::fs::Permissions::from_mode(0o600))
                .map_err(|err| io_err(temporary, err))?;
        }
        // A read-only handle cannot flush a file on Windows, so open it for writing.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(temporary)
            .map_err(|err| io_err(temporary, err))?;
        let synced = file.sync_all().map_err(|err| io_err(temporary, err));
        drop(file);
        synced
    };
    copy().map_err(PrepareError::Copy)?;
    let options = StoreOptions::beside(temporary, Timestamp::now());
    let store = Store::open(temporary, options)
        .await
        .map_err(|err| PrepareError::Open(RestoreError::Open(err)))?;
    let applied = applied_count(&store).await.unwrap_or(installed);
    store.close().await;
    Ok(applied.saturating_sub(installed))
}

/// Renames the current database and its SQLite companions to `before`, returning where
/// the database itself went. Nothing is left moved if any rename fails.
fn move_aside(database: &Path, before: &Path) -> Result<Option<PathBuf>, RestoreError> {
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (source, tail) in companions(database) {
        if !source.exists() {
            continue;
        }
        let destination = suffix(before, tail);
        if let Err(err) = std::fs::rename(&source, &destination) {
            for (from, to) in moved.iter().rev() {
                let _ = std::fs::rename(from, to);
            }
            return Err(io_err(&source, err));
        }
        moved.push((destination, source));
    }
    Ok(moved
        .iter()
        .find(|(_, source)| source == database)
        .map(|(destination, _)| destination.clone()))
}

/// Renames the prepared copy over `database`, with any SQLite side files the open left.
fn swap_in(temporary: &Path, database: &Path) -> Result<(), RestoreError> {
    for tail in ["-wal", "-shm"] {
        let side = suffix(temporary, tail);
        if side.exists() {
            let _ = std::fs::rename(&side, suffix(database, tail));
        }
    }
    std::fs::rename(temporary, database).map_err(|err| io_err(temporary, err))?;
    sync_dir(database);
    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &Path) {
    if let Some(dir) = path.parent()
        && let Ok(handle) = std::fs::File::open(dir)
    {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) {}
