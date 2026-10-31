//! Backups through `VACUUM INTO`: a consistent copy taken while the store stays open.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use sqlx::SqliteConnection;

use crate::db::{Store, create_private_dir};
use crate::error::StoreOpenError;

/// Nightly backups are named `domovoi-<time>.db`.
pub const BACKUP_PREFIX: &str = "domovoi-";

fn stamp(at: Timestamp) -> String {
    at.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// The name of the backup taken before migrations.
#[must_use]
pub fn pre_migrate_name(at: Timestamp) -> String {
    format!("pre-migrate-{}.db", stamp(at))
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

fn io(path: &Path, err: impl std::fmt::Display) -> StoreOpenError {
    StoreOpenError::Io {
        path: path.to_owned(),
        reason: err.to_string(),
    }
}

impl Store {
    /// Writes a backup to `dir` named for `now`, then deletes all but the newest
    /// `keep_backups` nightly backups there. Returns the new file.
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
        prune(dir, self.options.keep_backups)?;
        Ok(target)
    }
}

/// Deletes the oldest nightly backups in `dir` beyond `keep`. Names sort by time.
fn prune(dir: &Path, keep: usize) -> Result<(), StoreOpenError> {
    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(".db"))
        })
        .collect();
    backups.sort();
    let excess = backups.len().saturating_sub(keep.max(1));
    for old in backups.into_iter().take(excess) {
        std::fs::remove_file(&old).map_err(|e| io(&old, e))?;
    }
    Ok(())
}
