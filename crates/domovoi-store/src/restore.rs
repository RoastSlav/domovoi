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

mod durable;
mod files;
mod steps;

pub use self::durable::fail_next_sync_for_tests;

use self::steps::{forced, move_aside, roll_back, simulated_not_durable, swap_in, undo_fails};

use self::durable::{Fence, Stage, sync_dir};
use self::files::{Moves, reserve, same_file, sidecars_present, suffix};

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
    /// The renames happened but the directory could not be flushed, so a crash could still
    /// undo them. The restore is rolled back where it can be.
    #[error("cannot make {stage} durable: {}: {reason}", path.display())]
    NotDurable {
        /// The stage whose renames were not recorded.
        stage: String,
        /// The directory that could not be flushed.
        path: PathBuf,
        /// What the operating system said.
        reason: String,
    },
    /// The restore could not be made durable and putting the files back did not finish
    /// either. Nothing was deleted; both failures are named.
    #[error(
        "{reason}; putting the files back was incomplete ({recovery}); \
         nothing was deleted, and the result is not durable ({durability})"
    )]
    RecoveryIncompleteNotDurable {
        /// Why the restore failed, including the durability failure.
        reason: String,
        /// Each file that could not be moved back, and why.
        recovery: String,
        /// The directory flush that failed after the rollback.
        durability: String,
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
/// fails, leaving its moves to be undone), `undo-database` (the install fails and the
/// rollback of the moved database fails too), `failed-database` (keeping a copy that would
/// not open fails at the database), `failed-database-with-wal` (the same, after writing a
/// sidecar so the rollback covers a move that really happened) or
/// `failed-database-with-wal-stuck` (the same, with that rollback forced to fail),
/// `sync-aside`, `sync-install` or `sync-rollback` (a directory flush fails).
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
    let mut temporary = reserve(&suffix(database, ".restore-tmp"))?;

    // Migrate the copy under its temporary name: a backup that cannot be opened leaves
    // the live database exactly as it was.
    let applied = match prepare(backup, temporary.path(), now, report.migrations).await {
        Ok(applied) => applied,
        Err(PrepareError::Copy(err)) => {
            // The name holds only this run's partial copy; nothing has been moved over it.
            temporary.discard();
            return Err(err);
        }
        Err(PrepareError::Open(err)) => {
            keep_failed(
                temporary.path(),
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
    let had_database = database.exists();
    let moving_aside = had_database || sidecars_present(database);
    let mut before = moving_aside
        .then(|| database.with_file_name(format!("{name}.before-restore-{stamp}")))
        .map(|base| reserve(&base))
        .transpose()?;
    let mut aside = Moves::default();
    let aside_fence = Fence::around(database, Stage::Aside);
    if let Err(err) = move_aside(&mut aside, database, before.as_mut(), fail_at) {
        let stuck = aside.undo(undo_fails(fail_at));
        // Only a name still holding this run's own placeholder may go; a database that
        // moved back, or one the rollback could not put back, stays.
        if let Some(before) = before.as_mut() {
            before.abandon();
        }
        return Err(roll_back(err, stuck, &aside_fence, fail_at));
    }
    // The renames that put the previous database aside are only real once the directory
    // says so. If it cannot be flushed, the new database is not installed at all.
    let aside_synced = if forced(fail_at, "sync", "aside") {
        Err(simulated_not_durable(&aside_fence))
    } else {
        aside_fence.commit()
    };
    if let Err(err) = aside_synced {
        let stuck = aside.undo(undo_fails(fail_at));
        if let Some(before) = before.as_mut() {
            before.abandon();
        }
        return Err(roll_back(err, stuck, &aside_fence, fail_at));
    }
    // Without a database, only its sidecars moved: the reserved name holds nothing and
    // would read as a preserved database, so it goes. The sidecars keep the same prefix,
    // which the log names, so the operator can find what was set aside.
    let previous = match before.as_mut() {
        // `aside` only moved the database itself when it was there to move.
        Some(before) if aside.still_moved_to(before.path()) => {
            before.filled();
            Some(before.to_path_buf())
        }
        Some(before) => {
            // Only sidecars moved, so the bare name holds nothing this run must keep.
            before.abandon();
            if !had_database {
                tracing::info!(
                    preserved = %before.path().display(),
                    "no database to replace; its SQLite sidecars were moved aside too",
                );
            }
            None
        }
        None => None,
    };
    let mut swap = Moves::default();
    let install_fence = Fence::around(database, Stage::Install);
    if let Err(err) = swap_in(&mut swap, temporary.path(), database, fail_at) {
        let mut stuck = swap.undo(undo_fails(fail_at));
        stuck.extend(aside.undo(undo_fails(fail_at)));
        return Err(roll_back(err, stuck, &install_fence, fail_at));
    }
    // The installation is only complete once the directory records it. If the flush
    // fails, everything goes back: a restore that cannot promise the new database is
    // there must not leave the old one somewhere else.
    let installed = if forced(fail_at, "sync", "install") {
        Err(simulated_not_durable(&install_fence))
    } else {
        install_fence.commit()
    };
    if let Err(err) = installed {
        let mut stuck = swap.undo(undo_fails(fail_at));
        stuck.extend(aside.undo(undo_fails(fail_at)));
        return Err(roll_back(err, stuck, &install_fence, fail_at));
    }
    // The prepared copy is now the live database.
    temporary.filled();
    Ok(RestoreReport {
        backup: report,
        previous,
        migrations_applied_after: applied,
    })
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
    let mut failed = match reserve(&database.with_file_name(wanted)) {
        Ok(reserved) => reserved,
        Err(err) => {
            tracing::warn!(
                kept = %temporary.display(),
                %err,
                "the restored database did not open; its copy stays where it was",
            );
            return;
        }
    };
    // A test can require a sidecar to be there before the moves, so the rollback runs over
    // one that really happened instead of relying on SQLite to leave one behind.
    // The whole point of these two steps is that a sidecar exists to be moved, so failing
    // to write it is a failure of the test path, not something to log and carry on from.
    if matches!(
        fail_at,
        Some("failed-database-with-wal" | "failed-database-with-wal-stuck")
    ) {
        let wal = suffix(temporary, "-wal");
        if std::fs::write(&wal, b"preset-sidecar").is_err()
            || !wal.is_file()
            || std::fs::read(&wal).ok().as_deref() != Some(b"preset-sidecar")
        {
            failed.filled();
            tracing::error!(
                path = %wal.display(),
                "the test could not place a sidecar; the move it was meant to cover did not run",
            );
            return;
        }
    }
    let mut moves = Moves::default();
    let moved = moves
        .perform_sidecars(temporary, failed.path())
        .and_then(|()| {
            if matches!(
                fail_at,
                Some(
                    "failed-database"
                        | "failed-database-with-wal"
                        | "failed-database-with-wal-stuck"
                )
            ) {
                return Err(RestoreError::Io {
                    path: temporary.to_owned(),
                    reason: "kept for inspection: simulated failure".to_owned(),
                });
            }
            moves.perform(temporary, failed.path())
        });
    match moved {
        Ok(()) => {
            // The name now holds the copy that would not open, so it is never removed as
            // a placeholder, even though a sidecar may be all that landed there.
            failed.filled();
            tracing::warn!(
                failed = %failed.path().display(),
                "the restored database did not open; it is kept for inspection",
            );
        }
        Err(err) => {
            // Put back whatever moved, so no file is left split from the database. A move
            // that stayed put means the name holds data; the placeholder goes only when
            // the rollback left nothing of this run's copy behind.
            // `failed-database-with-wal-stuck` gives up on the sidecar, so a file really
            // stays at the failed name and the report has to name it.
            let stuck = moves.undo_with(
                undo_fails,
                fail_at == Some("failed-database-with-wal-stuck"),
            );
            if stuck.is_empty() && !moves.moved_to(failed.path()) {
                failed.abandon();
            } else {
                failed.filled();
            }
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
    // The copy and the entry naming it are flushed here, before the live database is
    // touched. The renames that follow are flushed by their own fences; this one only
    // promises the prepared copy is on disk.
    let directory = temporary
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    sync_dir(directory).map_err(|reason| {
        PrepareError::Copy(RestoreError::NotDurable {
            stage: "preparing the restored copy".to_owned(),
            path: directory.to_path_buf(),
            reason,
        })
    })?;
    let options = StoreOptions::beside(temporary, now);
    let store = Store::open(temporary, options)
        .await
        .map_err(|err| PrepareError::Open(RestoreError::Open(err)))?;
    let counted = applied_count(&store).await;
    store.close().await;
    let applied = counted.map_err(|err| PrepareError::Open(RestoreError::Open(err)))?;
    Ok(applied.saturating_sub(installed))
}
