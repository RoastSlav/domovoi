//! Runs on every guarded host: keeps the outbound link to the hub, schedules built-in checks,
//! re-validates and serves tool and skill requests through the runner, and spools signals
//! while the hub is unreachable. See ARCHITECTURE.md §7.2 and §8.
//!
//! - [`enroll`](mod@enroll) trades a one-time token for an [`identity::Identity`] and pins the hub's
//!   certificate.
//! - [`link`] keeps a mutually authenticated WebSocket over pinned TLS, reconnecting with
//!   backoff.
//! - [`scheduler`] runs checks and turns observations into signals; [`spool`] holds them
//!   on disk until the hub acknowledges them.
//! - [`requests`] re-validates every request against the local catalogue and policy before
//!   the runner sees it.
//! - [`runtime::AgentRuntime`] wires it all together.

pub mod banner;
pub mod boot;
pub mod checks;
pub mod config_file;
pub mod container;
pub mod containment;
pub mod docker;
pub mod dry_run;
pub mod enroll;
pub mod error;
pub mod facts;
pub mod fs_atomic;
pub mod helper_client;
pub mod identity;
pub mod journal;
pub mod link;
pub mod local;
pub mod overrides;
pub mod packages;
pub mod production;
pub mod relay;
pub mod requests;
pub mod runtime;
pub mod scheduler;
pub mod skill_runner;
pub mod spool;
pub mod state_owner;
pub mod store;
pub mod time;
pub mod unattended;

pub use boot::{BootSnapshot, BootSnapshotError, boot_snapshot};
pub use docker::DockerControl;
pub use domovoi_proto::EnrollToken;
pub use enroll::{EnrollEnv, enroll};
pub use error::{AgentError, EnrollError, JournalError, LinkError, StateError};
pub use identity::Identity;
pub use journal::{Journal, Metadata};
pub use runtime::{AgentDeps, AgentRuntime};
