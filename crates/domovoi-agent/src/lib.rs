//! The Domovoi agent runtime.
//!
//! Runs on every guarded host: keeps the outbound link to the hub, schedules
//! built-in checks, re-validates and serves tool and skill requests through
//! the runner, and spools signals while the hub is unreachable. See
//! `ARCHITECTURE.md` §7.2 and §8.
