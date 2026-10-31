//! Skill manifests and sandboxed process execution.
//!
//! The only crate allowed to spawn processes (ARCHITECTURE.md R3, ADR 0004).
//! Commands are argv vectors built from validated parameters, run as a
//! dedicated user with a cleared environment, a timeout that kills the whole
//! process group, capped output and redaction. Never a shell.
//!
//! - [`SandboxExecutor`] implements the `ProcessExecutor` port.
//! - [`SkillCatalog`] loads and checks the skills installed on this host and runs them.
//! - [`skill_run`] builds the argv for a skill run and hashes it for the audit log.

mod catalog;
mod error;
pub mod exec;
pub mod skill_run;

pub use catalog::{LoadedSkill, SkillCatalog};
pub use error::{CatalogIssue, RunError};
pub use exec::SandboxExecutor;
