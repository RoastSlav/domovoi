//! Backups through `VACUUM INTO`: a consistent copy taken while the store stays open,
//! checked before older copies are pruned.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use sqlx::SqliteConnection;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

use crate::db::{MIGRATOR, Store, create_private_dir};
use crate::error::StoreOpenError;

/// Nightly backups are named `domovoi-<time>.db`.
pub const BACKUP_PREFIX: &str = "domovoi-";

/// Pre-migration backups are named `pre-migrate-<time>.db`.
pub const PRE_MIGRATE_PREFIX: &str = "pre-migrate-";

fn stamp(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// The name of the backup taken before migrations.
#[must_use]
pub fn pre_migrate_name(at: Timestamp) -> String {
    format!("{PRE_MIGRATE_PREFIX}{}.db", stamp(at))
}

/// Copies the database to `target`, which must not exist yet.
pub(crate) async fn vacuum_into(
    conn: &mut SqliteConnection,
    target: &Path,
) -> Result<(), sqlx::Error> {
    sqlx::query("VACUUM INTO ?")
        .bind(target.to_string_lossy().into_owned())
        .execute(conn)
        .await
        .map(|_| ())
}

pub(crate) fn io(path: &Path, err: impl std::fmt::Display) -> StoreOpenError {
    StoreOpenError::Io {
        path: path.to_owned(),
        reason: err.to_string(),
    }
}

/// How thoroughly to check a database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// `PRAGMA quick_check`: a fast look for damage.
    Quick,
    /// `PRAGMA integrity_check`: every page and index.
    Full,
}

impl Depth {
    fn pragma(self) -> &'static str {
        match self {
            Self::Quick => "PRAGMA quick_check",
            Self::Full => "PRAGMA integrity_check",
        }
    }
}

/// What a verified backup is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupReport {
    /// How many migrations the file records as applied.
    pub migrations: usize,
    /// The highest migration version in the file.
    pub newest_migration: i64,
    /// The file's size.
    pub size_bytes: u64,
}

/// Why a database file is not a backup that can be restored.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// The file could not be read.
    #[error("{}: {reason}", path.display())]
    Io {
        /// The file involved.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// SQLite refused to open the file.
    #[error("cannot read the backup: {0}")]
    Unreadable(String),
    /// The integrity check found damage.
    #[error("the database failed its integrity check: {0}")]
    Corrupt(String),
    /// The file has no migration history.
    #[error("the backup has no migration history; it is not a Domovoi database")]
    NotDomovoi,
    /// The file was written by a newer build than this one.
    #[error("the backup was made by a newer Domovoi (migration {0}); upgrade before restoring it")]
    Newer(i64),
    /// A migration in the file did not finish.
    #[error("the backup has a migration that did not finish ({0})")]
    Unfinished(i64),
}

/// Opens `path` read-only (never creates it) and checks it.
pub async fn verify_backup(path: &Path, depth: Depth) -> Result<BackupReport, BackupError> {
    let metadata = std::fs::metadata(path).map_err(|err| BackupError::Io {
        path: path.to_owned(),
        reason: err.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(BackupError::Io {
            path: path.to_owned(),
            reason: "not a file".to_owned(),
        });
    }
    if metadata.len() == 0 {
        return Err(BackupError::Corrupt("the file is empty".to_owned()));
    }
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await
        .map_err(|err| BackupError::Unreadable(err.to_string()))?;
    let checked = check(&mut conn, depth).await;
    let _ = conn.close().await;
    let (migrations, newest_migration) = checked?;
    Ok(BackupReport {
        migrations,
        newest_migration,
        size_bytes: metadata.len(),
    })
}

/// The checks that need the open file, in order; returns the migration count and its
/// highest version.
async fn check(conn: &mut SqliteConnection, depth: Depth) -> Result<(usize, i64), BackupError> {
    let rows: Vec<(String,)> = sqlx::query_as(depth.pragma())
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| BackupError::Unreadable(err.to_string()))?;
    let lines: Vec<String> = rows.into_iter().map(|(line,)| line).collect();
    if lines.len() != 1 || !lines[0].eq_ignore_ascii_case("ok") {
        let detail: Vec<&str> = lines.iter().take(5).map(String::as_str).collect();
        return Err(BackupError::Corrupt(detail.join("; ")));
    }
    let has_table: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|err| BackupError::Unreadable(err.to_string()))?;
    if has_table.is_none() {
        return Err(BackupError::NotDomovoi);
    }
    let rows: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut *conn)
            .await
            .map_err(|err| BackupError::Unreadable(err.to_string()))?;
    if rows.is_empty() {
        return Err(BackupError::NotDomovoi);
    }
    let known: Vec<i64> = MIGRATOR.iter().map(|migration| migration.version).collect();
    let mut newest = 0;
    for (version, success) in &rows {
        newest = newest.max(*version);
        if !success {
            return Err(BackupError::Unfinished(*version));
        }
        if !known.contains(version) {
            return Err(BackupError::Newer(*version));
        }
    }
    Ok((rows.len(), newest))
}

/// Deletes `path` after a failed verification, so a bad copy never counts as a backup.
fn verify_or_remove(path: &Path) -> Result<(), StoreOpenError> {
    std::fs::remove_file(path).map_err(|err| io(path, err))?;
    Err(StoreOpenError::Verify {
        path: path.to_owned(),
        reason: "the new backup did not verify; it was removed and older backups were kept"
            .to_owned(),
    })
}

impl Store {
    /// Writes a backup to `dir` named for `now`, checks it, then deletes all but the
    /// newest `keep_backups` nightly backups and `keep_pre_migrate` pre-migration
    /// backups there. Returns the new file.
    ///
    /// A backup that does not verify is deleted and [`StoreOpenError::Verify`] returned,
    /// so a bad copy never displaces a good one.
    pub async fn backup_to(&self, dir: &Path, now: Timestamp) -> Result<PathBuf, StoreOpenError> {
        create_private_dir(dir)?;
        let target = dir.join(format!("{BACKUP_PREFIX}{}.db", stamp(now)));
        let mut conn = self
            .write
            .acquire()
            .await
            .map_err(|e| StoreOpenError::Database(e.to_string()))?;
        vacuum_into(&mut conn, &target)
            .await
            .map_err(|e| io(&target, e))?;
        drop(conn);
        verify_or_track(&target).await?;
        prune(dir, self.options.keep_backups)?;
        prune_pre_migrate(dir, self.options.keep_pre_migrate)?;
        Ok(target)
    }
}

/// Verifies a new backup, removing it and failing when it does not hold.
async fn verify_or_track(target: &Path) -> Result<(), StoreOpenError> {
    match verify_backup(target, Depth::Quick).await {
        Ok(_) => Ok(()),
        Err(err) => {
            tracing::warn!(backup = %target.display(), %err, "a new backup did not verify");
            verify_or_remove(target)
        }
    }
}

/// Deletes the oldest nightly backups in `dir` beyond `keep`. Names sort by time.
fn prune(dir: &Path, keep: usize) -> Result<(), StoreOpenError> {
    prune_matching(dir, BACKUP_PREFIX, ".db", keep)
}

/// Deletes the oldest `pre-migrate-*.db` files in `dir` beyond `keep` (at least 1 is
/// always kept). Names sort by time, so the newest copy is kept even with `keep = 0`.
pub(crate) fn prune_pre_migrate(dir: &Path, keep: usize) -> Result<(), StoreOpenError> {
    prune_matching(dir, PRE_MIGRATE_PREFIX, ".db", keep)
}

/// [`prune_pre_migrate`] over a directory that may hold only pre-migration copies, for
/// tests that do not need an open store.
#[doc(hidden)]
pub fn prune_pre_migrate_for_tests(dir: &Path, keep: usize) -> Result<(), StoreOpenError> {
    prune_pre_migrate(dir, keep)
}

fn prune_matching(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    keep: usize,
) -> Result<(), StoreOpenError> {
    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(suffix))
        })
        .collect();
    backups.sort();
    let excess = backups.len().saturating_sub(keep.max(1));
    for old in backups.into_iter().take(excess) {
        std::fs::remove_file(&old).map_err(|e| io(&old, e))?;
    }
    Ok(())
}
