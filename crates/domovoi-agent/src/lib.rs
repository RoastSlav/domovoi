//! Runs on every guarded host: keeps the outbound link to the hub, schedules built-in checks,
//! re-validates and serves tool and skill requests through the runner, and spools signals
//! while the hub is unreachable. See ARCHITECTURE.md §7.2 and §8.
//!
//! - [`enroll`] trades a one-time token for an [`identity::Identity`] and pins the hub's
//!   certificate.
//! - [`link`] keeps a mutually authenticated WebSocket over pinned TLS, reconnecting with
//!   backoff.
//! - [`scheduler`] runs checks and turns observations into signals; [`spool`] holds them
//!   on disk until the hub acknowledges them.
//! - [`requests`] re-validates every request against the local catalogue and policy before
//!   the runner sees it.
//! - [`runtime::AgentRuntime`] wires it all together.

pub mod checks;
pub mod enroll;
pub mod error;
pub mod facts;
pub mod fs_atomic;
pub mod identity;
pub mod link;
pub mod requests;
pub mod runtime;
pub mod scheduler;
pub mod spool;
pub mod store;
pub mod time;

pub use enroll::{EnrollEnv, enroll};
pub use error::{AgentError, EnrollError, LinkError, StateError};
pub use identity::Identity;
pub use runtime::{AgentDeps, AgentRuntime};
