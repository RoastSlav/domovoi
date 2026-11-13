//! Implements the repository and audit ports from `domovoi-core` on SQLite (WAL) via sqlx,
//! with embedded append-only migrations. See ARCHITECTURE.md §9 and ADR 0005.
//!
//! - [`Store::open`] creates the database directory, checks for FTS5, backs up a database
//!   that has pending migrations and runs them.
//! - [`Store`] implements every repository port; one connection writes, four read.
//! - The audit log is append-only in the database itself: triggers reject `UPDATE` and
//!   `DELETE` (INV-7).
//! - [`Store::backup_to`] takes a `VACUUM INTO` backup; [`Store::purge`] applies the
//!   [`RetentionPolicy`].

mod backup;
mod codec;
mod db;
mod error;
mod fts;
mod purge;
mod repos;

pub use backup::{BACKUP_PREFIX, pre_migrate_name};
pub use db::{BUSY_TIMEOUT, MIGRATOR, READ_CONNECTIONS, Store, StoreOptions, needs_backup};
pub use error::StoreOpenError;
pub use purge::{PurgeReport, RetentionPolicy};
pub use repos::SNIPPET_CHARS;
