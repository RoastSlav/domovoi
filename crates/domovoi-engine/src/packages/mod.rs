//! Installing signed packages on hosts: the install service.
//!
//! An owner's install or uninstall request becomes one job with one target per host. A worker
//! relays the archive and its signature file to the host's agent, hands the agent one
//! `agent.package.*` request and records what the agent answers. The agent is the authority
//! over what it installs; the hub never opens an archive and never decides which keys a host
//! trusts (ADR 0029).
//!
//! - `plan` shows what a request would do per host and binds it to plan hashes.
//! - `service` holds the handle that creates, retries and cancels jobs and refreshes a
//!   host's inventory, and the task that runs the worker.
//! - `execute` is the one place that talks to an agent about packages; `worker` is the one
//!   place that calls it.
//! - `settle` stores every state change as one transaction with its audit and journal rows;
//!   `reconcile` settles dispatched targets from the agent's journal.

mod audit;
mod deps;
mod error;
mod execute;
mod gate;
mod inventory;
mod plan;
mod reconcile;
mod relay;
mod service;
mod settle;
mod worker;

pub use deps::{PackageDeps, PackageSettings};
pub use error::{PackageError, PackageExecError};
pub use inventory::EnableAnswer;
pub use plan::{HostPlan, InstallPlan, PlanRequest};
pub use service::{InstallService, JobRequest, PackageHandle};

/// The actor of everything the install worker records.
pub const SYSTEM_PACKAGES: &str = "packages";

#[cfg(test)]
mod tests;
