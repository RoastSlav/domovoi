//! Errors from opening the store, and mapping SQLite errors onto `StoreError`.

use std::path::PathBuf;

use domovoi_core::ports::StoreError;

/// Why the store could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum StoreOpenError {
    /// The database directory or a backup could not be created.
    #[error("{}: {reason}", path.display())]
    Io {
        /// The path involved.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// SQLite could not open or configure the database.
    #[error("cannot open the database: {0}")]
    Database(String),
    /// This SQLite build lacks FTS5, which search needs.
    #[error("this SQLite build has no FTS5 support; Domovoi needs FTS5 for search")]
    NoFts5,
    /// A migration failed; the pre-migration backup is untouched.
    #[error("database migration failed: {0}")]
    Migrate(String),
}

/// Maps a query error: unique violations are conflicts, everything else is a backend error.
pub(crate) fn db(err: sqlx::Error) -> StoreError {
    match &err {
        sqlx::Error::Database(e) if e.is_unique_violation() => {
            StoreError::Conflict(e.message().to_owned())
        }
        _ => StoreError::Backend(err.to_string()),
    }
}

/// Maps a query error where a foreign key violation means the parent row is missing.
pub(crate) fn db_parent(err: sqlx::Error) -> StoreError {
    match &err {
        sqlx::Error::Database(e) if e.is_foreign_key_violation() => StoreError::NotFound,
        _ => db(err),
    }
}

/// A stored value that does not decode into its domain type.
pub(crate) fn corrupt(what: &str, err: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(format!("stored {what} is unreadable: {err}"))
}
