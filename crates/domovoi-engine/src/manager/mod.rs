//! The single-writer incident manager (ADR 0011).
//!
//! One task owns every incident change. Commands arrive on a bounded channel; for each the
//! manager loads what it needs, calls the pure [`decide`], applies the writes through the
//! repositories in order and only then publishes the events. A failed write publishes
//! nothing and fails the command.

mod actor;
mod annotate;
mod command;
mod decide;
mod handle;
mod redact;

pub use actor::{IncidentManager, ManagerDeps};
pub use command::{Command, CommandError};
pub use decide::{Decision, State, Write, decide};
pub use handle::{COMMAND_QUEUE, ManagerHandle};
