//! The Domovoi workspace's architecture checks.
//!
//! The crate exists for `cargo xtask`, but the checker is a library so the engine and hub gate
//! tests can call the one implementation of the execution-boundary rules instead of carrying
//! their own scans.
//!
//! xtask is not part of the product; the checker runs `cargo metadata` and `git` directly and
//! the tool prints its report, so the console and process rules that guard the product crates
//! are set off here, exactly as the tool's binary already does.

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::disallowed_methods)]

pub mod arch;
