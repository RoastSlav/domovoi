//! The agent's side of the skill runner: the socket client, the route that decides where a
//! skill runs, the shared status and the status probe.
//!
//! With the privilege boundary in effect (the agent is not root) skills run only through
//! the runner, as the skill account; a runner that is not `ok` makes every skill unavailable
//! and never falls back to a local run. A root agent keeps the labelled legacy path.

mod client;
mod route;
mod state;
mod status;

pub use client::{ClientError, RunnerClient};
pub use route::{MAX_CONCURRENT_RUNS, RouteError, RunnerRoute, SkillRoute};
pub use state::RunnerState;
pub use status::{
    ExpectedDirs, REASON_PROTOCOL_MISMATCH, REASON_ROOTS_DIFFER, REASON_SOCKET_PERMISSION,
    REASON_UNREACHABLE, from_error, probe_runner_status,
};
