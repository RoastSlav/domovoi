//! The single-writer incident manager (ADR 0011).
//!
//! One task owns every incident change. Commands arrive on a bounded channel; for each the
//! manager loads what it needs, calls the pure [`decide`], commits every write as one
//! transaction and only then publishes the events. A failed commit publishes nothing and
//! fails the command; a version conflict reloads and decides once more.

mod actor;
mod annotate;
mod command;
mod commit;
mod decide;
mod dismiss;
mod handle;
mod redact;

pub use actor::{IncidentManager, ManagerDeps};
pub use command::{Command, CommandError};
pub use decide::{Decision, State, Write, decide};
pub use handle::{COMMAND_QUEUE, ManagerHandle};
