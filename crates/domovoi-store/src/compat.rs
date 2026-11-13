//! Schema compatibility: which binary may read this database.
//!
//! A `schema_compat` row records the lowest binary version that can read the schema. A binary
//! older than that refuses to start, and a binary that knows fewer migrations than the database
//! ignores the unknown ones instead of failing.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::{ConnectOptions, Connection};

use crate::error::StoreOpenError;

/// Lowest binary version that can read a database written by this code.
///
/// Raise it in the same change as any migration an older binary cannot read: a drop, a rename or
/// a changed meaning.
pub const MIN_READER: &str = "0.0.0";

/// The version of this binary.
#[must_use]
pub const fn binary_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A `major.minor.patch` version, ignoring any `-` or `+` suffix.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split_once(['-', '+']).map_or(text, |(core, _)| core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// The larger of two versions, by numeric comparison.
fn larger<'a>(stored: &'a str, minimum: &'a str) -> Result<&'a str, StoreOpenError> {
    let stored_version = parse_version(stored).ok_or_else(unreadable)?;
    let minimum_version = parse_version(minimum).ok_or_else(unreadable)?;
    Ok(if stored_version >= minimum_version {
        stored
    } else {
        minimum
    })
}

fn unreadable() -> StoreOpenError {
    StoreOpenError::Database("schema_compat.min_reader is unreadable".to_owned())
}

/// Refuses a database whose recorded reader version is newer than this binary.
///
/// Called before any backup or migration, so a refused database is left untouched.
pub(crate) async fn check(path: &Path) -> Result<(), StoreOpenError> {
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await
        .map_err(|e| StoreOpenError::Database(e.to_string()))?;
    let has_table: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'schema_compat'",
    )
    .fetch_optional(&mut conn)
    .await
    .map_err(|e| StoreOpenError::Database(e.to_string()))?;
    if has_table.is_some() {
        let stored: Option<(String,)> =
            sqlx::query_as("SELECT min_reader FROM schema_compat WHERE id = 1")
                .fetch_optional(&mut conn)
                .await
                .map_err(|e| StoreOpenError::Database(e.to_string()))?;
        if let Some((min_reader,)) = stored {
            let needed = parse_version(&min_reader).ok_or_else(unreadable)?;
            let binary = parse_version(binary_version()).ok_or_else(unreadable)?;
            if binary < needed {
                conn.close()
                    .await
                    .map_err(|e| StoreOpenError::Database(e.to_string()))?;
                return Err(StoreOpenError::SchemaTooNew {
                    min_reader,
                    binary: binary_version().to_owned(),
                });
            }
        }
    }
    conn.close()
        .await
        .map_err(|e| StoreOpenError::Database(e.to_string()))
}

/// Records the reader version after migrations.
///
/// Writes only when migrations were pending, the database was new, or no row exists. An older
/// binary opening a newer database must not overwrite `written_by`.
pub(crate) async fn record(
    pool: &SqlitePool,
    migrated: bool,
    written_at: i64,
) -> Result<(), StoreOpenError> {
    let stored: Option<(String,)> =
        sqlx::query_as("SELECT min_reader FROM schema_compat WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(|e| StoreOpenError::Database(e.to_string()))?;
    if !migrated && stored.is_some() {
        return Ok(());
    }
    let min_reader = match &stored {
        Some((stored,)) => larger(stored, MIN_READER)?.to_owned(),
        None => MIN_READER.to_owned(),
    };
    sqlx::query(
        "INSERT INTO schema_compat (id, min_reader, written_by, written_at) VALUES (1, ?, ?, ?) \
         ON CONFLICT (id) DO UPDATE SET min_reader = excluded.min_reader, \
         written_by = excluded.written_by, written_at = excluded.written_at",
    )
    .bind(&min_reader)
    .bind(binary_version())
    .bind(written_at)
    .execute(pool)
    .await
    .map_err(|e| StoreOpenError::Database(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_not_as_text() {
        assert!(parse_version("0.1.9") < parse_version("0.1.10"));
        assert_eq!(parse_version("1.0.0-rc.1"), parse_version("1.0.0"));
        assert_eq!(parse_version("1.0.0+build"), parse_version("1.0.0"));
        assert_eq!(parse_version("garbage"), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(larger("0.0.1", MIN_READER).unwrap(), "0.0.1");
        assert_eq!(larger(MIN_READER, "0.0.1").unwrap(), "0.0.1");
    }
}
