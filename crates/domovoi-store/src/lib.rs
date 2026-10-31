//! SQLite persistence adapter.
//!
//! Implements the repository and audit ports from `domovoi-core` on SQLite
//! (WAL mode) via `sqlx`, with embedded append-only migrations. See
//! `ARCHITECTURE.md` §9 and ADR 0005.
