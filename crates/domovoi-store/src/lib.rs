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

mod audit_chain;
mod audit_verify;
mod backup;
mod codec;
mod compat;
mod db;
mod error;
mod experience;
mod facts;
mod fts;
mod health;
mod integrity;
mod policy;
mod purge;
mod repos;
mod restore;
mod uow;

pub use backup::{
    BACKUP_PREFIX, BackupError, BackupReport, Depth, PRE_MIGRATE_PREFIX, pre_migrate_name,
    prune_pre_migrate_for_tests, verify_backup, verify_or_track_for_tests,
    verify_or_tracked_for_tests,
};
pub use compat::MIN_READER;
pub use db::{BUSY_TIMEOUT, MIGRATOR, READ_CONNECTIONS, Store, StoreOptions, needs_backup};
pub use error::StoreOpenError;
pub use purge::{FTS_CLEANUP, PURGED_TABLES, PurgeReport, RetentionPolicy};
pub use repos::SNIPPET_CHARS;
pub use restore::{
    RestoreError, RestoreReport, fail_next_sync_for_tests, restore_from_backup,
    restore_from_backup_failing_at,
};
