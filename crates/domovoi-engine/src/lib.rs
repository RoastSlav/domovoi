//! Application services for Domovoi.
//!
//! Correlation, the single-writer incident manager, notification policy,
//! triage orchestration, approvals and digests. Depends only on
//! `domovoi-core` ports; never performs I/O directly. See `ARCHITECTURE.md` §7
//! and ADR 0011.
