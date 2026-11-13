//! Opening the database: directory, connection settings, FTS5, backup, migrations.

use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{ConnectOptions, Connection};

use crate::backup::{pre_migrate_name, vacuum_into};
use crate::codec::ms;
use crate::compat;
use crate::error::StoreOpenError;
use crate::integrity;
/// The embedded migrations, in order.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// How long a connection waits for another writer before giving up.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Connections in the read pool.
pub const READ_CONNECTIONS: u32 = 4;

/// How to open the store.
#[derive(Debug, Clone)]
pub struct StoreOptions {
    /// Where backups go, including the one taken before migrations.
    pub backup_dir: PathBuf,
    /// How many nightly backups [`Store::backup_to`] keeps.
    pub keep_backups: usize,
    /// When the store is opened, which names the pre-migration backup.
    pub now: Timestamp,
}

impl StoreOptions {
    /// Options with backups next to the database in `backups/`, keeping 7.
    #[must_use]
    pub fn beside(database: &Path, now: Timestamp) -> Self {
        let dir = database.parent().unwrap_or(Path::new("."));
        Self {
            backup_dir: dir.join("backups"),
            keep_backups: 7,
            now,
        }
    }
}

/// The SQLite store. Cheap to clone; every clone shares the pools.
#[derive(Debug, Clone)]
pub struct Store {
    pub(crate) read: SqlitePool,
    pub(crate) write: SqlitePool,
    pub(crate) options: StoreOptions,
}

/// Whether a migration in `available` has not been applied yet.
#[must_use]
pub fn needs_backup(applied: &[i64], available: &[i64]) -> bool {
    available.iter().any(|v| !applied.contains(v))
}

fn io(path: &Path, err: impl std::fmt::Display) -> StoreOpenError {
    StoreOpenError::Io {
        path: path.to_owned(),
        reason: err.to_string(),
    }
}

fn database(err: impl std::fmt::Display) -> StoreOpenError {
    StoreOpenError::Database(err.to_string())
}

/// Creates `dir` and any missing parents, readable by the owner only.
pub(crate) fn create_private_dir(dir: &Path) -> Result<(), StoreOpenError> {
    if dir.as_os_str().is_empty() || dir.exists() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).map_err(|e| io(dir, e))
}

fn connect_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT)
}

/// The migration versions already applied to the database at `path`.
async fn applied_versions(path: &Path) -> Result<Vec<i64>, StoreOpenError> {
    let mut conn = connect_options(path).connect().await.map_err(database)?;
    let has_table: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(&mut conn)
    .await
    .map_err(database)?;
    let versions = if has_table.is_some() {
        sqlx::query_as::<_, (i64,)>("SELECT version FROM _sqlx_migrations WHERE success = 1")
            .fetch_all(&mut conn)
            .await
            .map_err(database)?
            .into_iter()
            .map(|(v,)| v)
            .collect()
    } else {
        Vec::new()
    };
    conn.close().await.map_err(database)?;
    Ok(versions)
}

impl Store {
    /// Opens (or creates) the database at `path` and brings its schema up to date.
    ///
    /// An existing database with pending migrations is first copied to
    /// `<backup_dir>/pre-migrate-<time>.db`.
    pub async fn open(path: &Path, options: StoreOptions) -> Result<Self, StoreOpenError> {
        if let Some(dir) = path.parent() {
            create_private_dir(dir)?;
        }
        let existed = path.metadata().is_ok_and(|m| m.len() > 0);
        let available: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
        let applied = if existed {
            applied_versions(path).await?
        } else {
            Vec::new()
        };
        let pending = needs_backup(&applied, &available);
        if existed {
            compat::check(path).await?;
        }
        if existed && pending {
            create_private_dir(&options.backup_dir)?;
            let target = options.backup_dir.join(pre_migrate_name(options.now));
            let mut conn = connect_options(path).connect().await.map_err(database)?;
            vacuum_into(&mut conn, &target)
                .await
                .map_err(|e| io(&target, e))?;
            conn.close().await.map_err(database)?;
            tracing::info!(backup = %target.display(), "backed up the database before migrating");
        }
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options(path))
            .await
            .map_err(database)?;
        sqlx::query("CREATE VIRTUAL TABLE IF NOT EXISTS temp.fts_probe USING fts5(x)")
            .execute(&write)
            .await
            .map_err(|_| StoreOpenError::NoFts5)?;
        let mut migrator = Migrator::with_migrations(MIGRATOR.iter().cloned().collect());
        migrator.set_ignore_missing(true);
        migrator
            .run(&write)
            .await
            .map_err(|e| StoreOpenError::Migrate(e.to_string()))?;
        compat::record(&write, !existed || pending, ms(options.now)).await?;
        crate::audit_chain::ensure_anchor(&write, options.now).await?;
        integrity::quick_check(&write).await?;
        let read = SqlitePoolOptions::new()
            .max_connections(READ_CONNECTIONS)
            .connect_with(connect_options(path))
            .await
            .map_err(database)?;
        Ok(Self {
            read,
            write,
            options,
        })
    }

    /// Runs `PRAGMA quick_check` on the database.
    ///
    /// A backup verification uses this too: an `Ok` means the file is sound.
    pub async fn quick_check(&self) -> Result<(), StoreOpenError> {
        integrity::quick_check(&self.write).await
    }

    /// Closes both pools, waiting for open connections.
    pub async fn close(&self) {
        self.write.close().await;
        self.read.close().await;
    }

    /// The writer pool, for tests that need raw SQL.
    #[doc(hidden)]
    #[must_use]
    pub fn pool_for_tests(&self) -> &SqlitePool {
        &self.write
    }
}
