//! Persistence ports, implemented by `domovoi-store` and by the in-memory fake in
//! `domovoi-testkit`.
//!
//! Rules shared by every repository:
//!
//! - `get*` methods return `Ok(None)` for a missing record; mutations of a missing record
//!   return [`StoreError::NotFound`].
//! - Inserting a record whose id already exists returns [`StoreError::Conflict`].
//! - Paged listings are newest first, ties broken by id descending. Cursors are opaque and
//!   stay valid when newer records are inserted: a page never repeats or skips a record
//!   that existed when the first page was read.
//! - [`StoreError::Backend`] means the store itself failed; nothing was written.

mod accounts;
mod actions;
mod audit;
mod hosts;
mod incidents;
mod knowledge;
mod notify;
mod triage;

pub use accounts::{SessionRepo, SettingsRepo, UserRepo};
pub use actions::{ActionFilter, ActionRepo};
pub use audit::{AuditEvent, AuditFilter, AuditLog};
pub use hosts::{EnrollmentToken, HostRepo};
pub use incidents::{IncidentFilter, IncidentRepo, IncidentTimelineEntry, SignalRepo};
pub use knowledge::{AckRepo, AdvisoryRepo, BaselineRepo, FactRepo, NoteHit, NoteRepo};
pub use notify::NotifyStateRepo;
pub use triage::{TriageCacheRepo, UsageRepo, UsageTotal};

/// Why a repository call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The record to change does not exist.
    #[error("not found")]
    NotFound,
    /// The write would break a uniqueness rule; the message names it.
    #[error("conflict: {0}")]
    Conflict(String),
    /// The store failed.
    #[error("store error: {0}")]
    Backend(String),
}

/// Which page of a listing to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// `None` for the first page, otherwise a [`Paged::next_cursor`] from the previous page.
    pub cursor: Option<String>,
    /// Maximum items; 0 is read as 1 and values above [`Page::MAX_LIMIT`] are capped.
    pub limit: u16,
}

impl Page {
    /// The largest page size a store returns.
    pub const MAX_LIMIT: u16 = 500;

    /// The first page with `limit` items.
    #[must_use]
    pub const fn first(limit: u16) -> Self {
        Self {
            cursor: None,
            limit,
        }
    }

    /// The limit after applying the 1..=[`MAX_LIMIT`](Self::MAX_LIMIT) bounds.
    #[must_use]
    pub fn effective_limit(&self) -> usize {
        usize::from(self.limit.clamp(1, Self::MAX_LIMIT))
    }
}

/// One page of a listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Paged<T> {
    /// The items, newest first.
    pub items: Vec<T>,
    /// Cursor for the next page; `None` when this is the last page.
    pub next_cursor: Option<String>,
}
