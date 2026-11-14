//! Installing a backup as the live database, keeping the one it replaces.
//!
//! The caller holds the data directory's lock: nothing runs this against a database a
//! hub is using. Nothing here deletes the previous database, and a failure puts every
//! file it moved back where it was, so an operator never ends without one.
//!
//! Order: verify the backup, prepare a migrated copy beside the database, move the
//! current database aside, and only then swap the copy in. A backup that does not open
//! therefore never touches the live database.

use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::backup::{BackupError, BackupReport, Depth, verify_backup};
use crate::db::{Store, StoreOptions};
use crate::error::StoreOpenError;

mod files;

use self::files::{
    Moves, companions, remove_if_empty, reserve, same_file, sidecars_present, suffix,
};

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
    /// The restored database could not be opened, migrated or counted.
    #[error(transparent)]
    Open(#[from] StoreOpenError),
    /// The backup is the database itself.
    #[error("the backup and the database are the same file")]
    SameFile,
    /// The restore failed and putting the files back did not finish. Nothing was
    /// deleted; the message names what is still out of place.
    #[error("{reason}; putting the files back was incomplete ({recovery}); nothing was deleted")]
    RecoveryIncomplete {
        /// Why the restore failed.
        reason: String,
        /// Each file that could not be moved back, and why.
        recovery: String,
    },
}

fn io_err(path: &Path, err: impl std::fmt::Display) -> RestoreError {
    RestoreError::Io {
        path: path.to_owned(),
        reason: err.to_string(),
    }
}

/// The migration count of an open store.
async fn applied_count(store: &Store) -> Result<usize, StoreOpenError> {
    sqlx::query_as::<_, (i64,)>("SELECT version FROM _sqlx_migrations WHERE success = 1")
        .fetch_all(store.pool_for_tests())
        .await
        .map(|rows| rows.len())
        .map_err(|err| StoreOpenError::Database(format!("cannot count the migrations: {err}")))
}

/// Installs `backup` as `database`. The caller holds the data-directory lock.
pub async fn restore_from_backup(
    database: &Path,
    backup: &Path,
    now: Timestamp,
) -> Result<RestoreReport, RestoreError> {
    restore(database, backup, now, None).await
}

/// The same restore with one file-system step forced to fail, so tests can drive the
/// recovery paths without depending on permissions an elevated run ignores. `step` is one
/// of `aside-wal`, `aside-shm`, `swap-wal`, `swap-shm`, `swap-database` (a step that
/// fails, leaving its moves to be undone) or `undo-database` (the install fails and the
/// rollback of the moved database fails too).
#[doc(hidden)]
pub async fn restore_from_backup_failing_at(
    database: &Path,
    backup: &Path,
    now: Timestamp,
    step: &str,
) -> Result<RestoreReport, RestoreError> {
    restore(database, backup, now, Some(step)).await
}

async fn restore(
    database: &Path,
    backup: &Path,
    now: Timestamp,
    fail_at: Option<&str>,
) -> Result<RestoreReport, RestoreError> {
    if same_file(database, backup) {
        return Err(RestoreError::SameFile);
    }
    let report = verify_backup(backup, Depth::Full).await?;
    let name = database
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let stamp = stamp(now);
    let temporary = reserve(&suffix(database, ".restore-tmp"))?;

    // Migrate the copy under its temporary name: a backup that cannot be opened leaves
    // the live database exactly as it was.
    let applied = match prepare(backup, &temporary, now, report.migrations).await {
        Ok(applied) => applied,
        Err(PrepareError::Copy(err)) => {
            // Clean up the placeholder or partial copy: the empty name would otherwise
            // make the next run skip it.
            for tail in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(suffix(&temporary, tail));
            }
            return Err(err);
        }
        Err(PrepareError::Open(err)) => {
            keep_failed(
                &temporary,
                database,
                &format!("{name}.failed-restore-{stamp}"),
                fail_at,
                undo_fails(fail_at),
            );
            return Err(err);
        }
    };

    // A database that exists is moved aside. So are sidecars without one: SQLite would
    // drop them on the next open, and they may be the only copy of a write in flight. A
    // name is reserved for neither without something to put in it, which would leave an
    // empty `.before-restore-*` file looking like a preserved database.
    let moving_aside = database.exists() || sidecars_present(database);
    let before = moving_aside
        .then(|| database.with_file_name(format!("{name}.before-restore-{stamp}")))
        .map(|base| reserve(&base))
        .transpose()?
        .unwrap_or_default();
    let mut aside = Moves::default();
    if let Err(err) = move_aside(&mut aside, database, &before, fail_at) {
        let stuck = aside.undo(undo_fails(fail_at));
        if moving_aside {
            remove_if_empty(&before);
        }
        return Err(recover(err, stuck));
    }
    // Without a database, only its sidecars moved: the reserved name holds nothing and
    // would read as a preserved database, so it goes. The sidecars keep the same prefix,
    // which the log names, so the operator can find what was set aside.
    if moving_aside {
        remove_if_empty(&before);
        if !database.exists() {
            tracing::info!(
                preserved = %before.display(),
                "no database to replace; its SQLite sidecars were moved aside too",
            );
        }
    }
    let mut swap = Moves::default();
    if let Err(err) = swap_in(&mut swap, &temporary, database, fail_at) {
        let mut stuck = swap.undo(undo_fails(fail_at));
        stuck.extend(aside.undo(undo_fails(fail_at)));
        return Err(recover(err, stuck));
    }
    let previous = aside.moved_to(&before).then_some(before);
    Ok(RestoreReport {
        backup: report,
        previous,
        migrations_applied_after: applied,
    })
}

/// Whether the file that first moves aside should fail to move back.
///
/// `undo-database` makes the install fail and then the rollback fail too, which is how a
/// test reaches [`RestoreError::RecoveryIncomplete`] without depending on permissions.
fn undo_fails(fail_at: Option<&str>) -> bool {
    fail_at == Some("undo-database")
}

fn stamp(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// Moves the copy that would not open to a name that says so, so it can be inspected.
///
/// The live database has not been touched at this point, so this never hides the original
/// error: it is best effort. A move that fails is undone, so the database and its sidecars
/// stay together at the temporary name rather than being split across two names.
fn keep_failed(
    temporary: &Path,
    database: &Path,
    wanted: &str,
    fail_at: Option<&str>,
    undo_fails: bool,
) {
    let failed = match reserve(&database.with_file_name(wanted)) {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(
                kept = %temporary.display(),
                %err,
                "the restored database did not open; its copy stays where it was",
            );
            return;
        }
    };
    let mut moves = Moves::default();
    let moved = moves.perform_sidecars(temporary, &failed).and_then(|()| {
        if fail_at == Some("failed-database") {
            return Err(RestoreError::Io {
                path: temporary.to_owned(),
                reason: "kept for inspection: simulated failure".to_owned(),
            });
        }
        moves.perform(temporary, &failed)
    });
    match moved {
        Ok(()) => tracing::warn!(
            failed = %failed.display(),
            "the restored database did not open; it is kept for inspection",
        ),
        Err(err) => {
            // Put back whatever moved, so no file is left split from the database. The
            // placeholder is still empty: the database only moves last.
            let stuck = moves.undo(undo_fails);
            remove_if_empty(&failed);
            if stuck.is_empty() {
                tracing::warn!(
                    kept = %temporary.display(),
                    %err,
                    "the restored database did not open; its copy stays where it was",
                );
            } else {
                tracing::warn!(
                    kept = %temporary.display(),
                    misplaced = %stuck.join("; "),
                    %err,
                    "the restored database did not open; some of its files could not be put back",
                );
            }
        }
    }
}

/// Why preparing the copy failed: while copying it, or while opening it.
enum PrepareError {
    Copy(RestoreError),
    Open(RestoreError),
}

/// Copies `backup` to `temporary`, makes it owner-only, syncs it, and opens it there so
/// pending migrations run. Returns how many ran, against the `installed` the backup
/// already recorded. `now` names the pre-migration copy the open may take.
async fn prepare(
    backup: &Path,
    temporary: &Path,
    now: Timestamp,
    installed: usize,
) -> Result<usize, PrepareError> {
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
    // The directory entry is made durable here, before the swap, so the swap itself has
    // no step left that could fail after the live database is replaced.
    sync_dir(temporary).map_err(PrepareError::Copy)?;
    let options = StoreOptions::beside(temporary, now);
    let store = Store::open(temporary, options)
        .await
        .map_err(|err| PrepareError::Open(RestoreError::Open(err)))?;
    let counted = applied_count(&store).await;
    store.close().await;
    let applied = counted.map_err(|err| PrepareError::Open(RestoreError::Open(err)))?;
    Ok(applied.saturating_sub(installed))
}

/// Moves the current database and its sidecars to `before`.
fn move_aside(
    moves: &mut Moves,
    database: &Path,
    before: &Path,
    fail_at: Option<&str>,
) -> Result<(), RestoreError> {
    for (source, tail) in companions(database) {
        if !source.exists() {
            continue;
        }
        if forced(fail_at, "aside", tail) {
            return Err(RestoreError::Io {
                path: source,
                reason: "cannot move it aside: simulated failure".to_owned(),
            });
        }
        moves.perform(&source, &suffix(before, tail))?;
    }
    Ok(())
}

/// Moves the prepared copy over `database`, with the SQLite side files its open left.
fn swap_in(
    moves: &mut Moves,
    temporary: &Path,
    database: &Path,
    fail_at: Option<&str>,
) -> Result<(), RestoreError> {
    for tail in ["-wal", "-shm"] {
        let side = suffix(temporary, tail);
        if !side.exists() {
            continue;
        }
        if forced(fail_at, "swap", tail) {
            return Err(RestoreError::Io {
                path: side,
                reason: "cannot install it: simulated failure".to_owned(),
            });
        }
        moves.perform(&side, &suffix(database, tail))?;
    }
    if forced(fail_at, "swap", "") {
        return Err(RestoreError::Io {
            path: temporary.to_owned(),
            reason: "cannot install it: simulated failure".to_owned(),
        });
    }
    moves.perform(temporary, database)
}

/// Whether the step `phase` with sidecar `tail` is the one a test asked to fail.
fn forced(fail_at: Option<&str>, phase: &str, tail: &str) -> bool {
    let wanted = match (phase, tail) {
        ("aside", "-wal") => "aside-wal",
        ("aside", "-shm") => "aside-shm",
        ("swap", "-wal") => "swap-wal",
        ("swap", "-shm") => "swap-shm",
        ("swap", "") => {
            // The rollback-failure step installs nothing either.
            return matches!(fail_at, Some("swap-database" | "undo-database"));
        }
        _ => return false,
    };
    fail_at == Some(wanted)
}

/// The error to report after a failed step, naming the moves that could not be undone.
fn recover(err: RestoreError, stuck: Vec<String>) -> RestoreError {
    if stuck.is_empty() {
        err
    } else {
        RestoreError::RecoveryIncomplete {
            reason: err.to_string(),
            recovery: stuck.join("; "),
        }
    }
}

/// Flushes the directory holding `path`, so a rename there survives a crash. Errors are
/// propagated: a restore that cannot make its copy durable must not go on to swap it in.
#[cfg(unix)]
fn sync_dir(path: &Path) -> Result<(), RestoreError> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let handle = std::fs::File::open(dir).map_err(|err| io_err(dir, err))?;
    handle.sync_all().map_err(|err| io_err(dir, err))
}

/// Windows has no portable directory fsync, so this is a no-op there (see the note).
#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> Result<(), RestoreError> {
    // Windows has no portable directory fsync. The file's own data is synced before the
    // swap and the renames are ordered, but a crash can still lose the directory entry;
    // the preserved previous database is what the operator falls back to.
    Ok(())
}
