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

/// How many collision suffixes an artifact name tries before the restore gives up.
const NAMES_TRIED: u32 = 16;

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

fn suffix(path: &Path, tail: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
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

/// `base`, then `base-2`, `base-3`, ... for each attempt.
fn numbered(base: &Path, attempt: u32) -> PathBuf {
    if attempt == 0 {
        base.to_path_buf()
    } else {
        suffix(base, &format!("-{}", attempt + 1))
    }
}

/// Claims a fresh name, never one that already exists.
///
/// The placeholder is created atomically with `create_new`, so two runs given the same
/// timestamp cannot take the same name and an earlier artifact is never replaced. A name
/// whose `-wal` or `-shm` is already there is skipped too: an interrupted restore may have
/// left those behind. The caller holds the data-directory lock, so nothing competes for
/// the name afterwards, and the rename that fills it replaces only this run's empty
/// placeholder.
fn reserve(base: &Path) -> Result<PathBuf, RestoreError> {
    for attempt in 0..NAMES_TRIED {
        let candidate = numbered(base, attempt);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => {
                if suffix(&candidate, "-wal").exists() || suffix(&candidate, "-shm").exists() {
                    let _ = std::fs::remove_file(&candidate);
                    continue;
                }
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(io_err(&candidate, err)),
        }
    }
    Err(RestoreError::Io {
        path: base.to_owned(),
        reason: format!("no free name after {NAMES_TRIED} attempts; move the old ones away"),
    })
}

/// Files moved during a restore, so a failure can put each one back exactly.
#[derive(Default)]
struct Moves {
    /// `(from, to)` for every move that succeeded, oldest first.
    done: Vec<(PathBuf, PathBuf)>,
}

impl Moves {
    /// Moves `from` to `to` and records it.
    fn perform(&mut self, from: &Path, to: &Path) -> Result<(), RestoreError> {
        std::fs::rename(from, to).map_err(|err| io_err(from, err))?;
        self.done.push((from.to_owned(), to.to_owned()));
        Ok(())
    }

    /// Moves the SQLite sidecars of `from` to those of `to`, where they exist.
    fn perform_sidecars(&mut self, from: &Path, to: &Path) -> Result<(), RestoreError> {
        for tail in ["-wal", "-shm"] {
            let source = suffix(from, tail);
            if source.exists() {
                self.perform(&source, &suffix(to, tail))?;
            }
        }
        Ok(())
    }

    /// Reverses every move, newest first. Returns one line per move it could not undo.
    fn undo(&mut self) -> Vec<String> {
        let mut stuck = Vec::new();
        while let Some((from, to)) = self.done.pop() {
            if let Err(err) = std::fs::rename(&to, &from) {
                stuck.push(format!(
                    "{} back to {}: {err}",
                    to.display(),
                    from.display()
                ));
            }
        }
        stuck
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
/// of `aside-wal`, `aside-shm`, `swap-wal`, `swap-shm` or `swap-database`.
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
            );
            return Err(err);
        }
    };

    let had_database = database.exists();
    let before = reserve(&database.with_file_name(format!("{name}.before-restore-{stamp}")))?;
    let mut aside = Moves::default();
    if let Err(err) = move_aside(&mut aside, database, &before, fail_at) {
        let stuck = aside.undo();
        if !had_database {
            let _ = std::fs::remove_file(&before);
        }
        return Err(recover(err, stuck));
    }
    let mut swap = Moves::default();
    if let Err(err) = swap_in(&mut swap, &temporary, database, fail_at) {
        let mut stuck = swap.undo();
        stuck.extend(aside.undo());
        return Err(recover(err, stuck));
    }
    Ok(RestoreReport {
        backup: report,
        previous: had_database.then_some(before),
        migrations_applied_after: applied,
    })
}

fn stamp(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// Moves the copy that would not open to a name that says so, so it can be inspected.
///
/// This is best effort on purpose: the live database has not been touched at this point,
/// so a failure here keeps the copy at its temporary name and the original error stands.
fn keep_failed(temporary: &Path, database: &Path, wanted: &str) {
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
    let moved = moves
        .perform_sidecars(temporary, &failed)
        .and_then(|()| moves.perform(temporary, &failed));
    match moved {
        Ok(()) => tracing::warn!(
            failed = %failed.display(),
            "the restored database did not open; it is kept for inspection",
        ),
        Err(err) => tracing::warn!(
            kept = %temporary.display(),
            %err,
            "the restored database did not open; its copy stays where it was",
        ),
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
        ("swap", "") => "swap-database",
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
