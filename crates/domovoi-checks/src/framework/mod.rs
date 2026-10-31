//! What a check is, what it may touch and what it reports.

mod report;
mod settings;

#[cfg(feature = "host")]
mod registry;

use async_trait::async_trait;
use domovoi_core::ports::{Clock, FsError, HostFs, ProcessError, ProcessExecutor};
use domovoi_core::{CheckKey, HostId, SignalClass};
use jiff::SignedDuration;

use crate::commands::{CommandError, Commands};

pub use report::{CheckReport, Observation, number_fact, round2};
pub use settings::{CheckSettings, CheckState};

#[cfg(feature = "host")]
pub use registry::{HostDeps, Registry};

/// Longest a single check run may take before [`run_bounded`] gives up on it.
pub const RUN_LIMIT: SignedDuration = SignedDuration::from_secs(20);

/// Why a check run produced no report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// A host file could not be read.
    #[error("reading {path}: {source}")]
    Fs {
        /// The host path.
        path: String,
        /// What went wrong.
        source: FsError,
    },
    /// A command template could not be built.
    #[error(transparent)]
    Command(#[from] CommandError),
    /// The executor could not run a command.
    #[error("running {program}: {source}")]
    Process {
        /// The program.
        program: String,
        /// What went wrong.
        source: ProcessError,
    },
    /// `[checks.<key>]` settings are invalid for this check.
    #[error("invalid settings: {0}")]
    Settings(String),
    /// The run took longer than [`RUN_LIMIT`].
    #[error("check did not finish within {}s", RUN_LIMIT.as_secs())]
    Timeout,
}

impl CheckError {
    /// A read failure at `path`.
    #[must_use]
    pub fn fs(path: &str, source: FsError) -> Self {
        Self::Fs {
            path: path.to_owned(),
            source,
        }
    }
}

/// Everything a check may use while it runs.
pub struct CheckContext<'a> {
    /// The host being checked.
    pub host: HostId,
    /// Read-only access to the host's files.
    pub fs: &'a dyn HostFs,
    /// Runs template-built commands; the only way a check reaches a process.
    pub exec: &'a dyn ProcessExecutor,
    /// Resolved command programs.
    pub commands: &'a Commands,
    /// The current time.
    pub clock: &'a dyn Clock,
    /// This check's `[checks.<key>]` settings.
    pub settings: &'a CheckSettings,
    /// This check's persistent state (cursors, previous boot id, ...), saved by the agent.
    pub state: &'a mut CheckState,
}

/// One built-in check.
///
/// A check never panics, reads through [`CheckContext::fs`], runs commands only through
/// [`CheckContext::exec`] with specs from [`crate::commands`], and keeps memory bounded:
/// command output is capped and files are read whole only when they are small.
#[async_trait]
pub trait Check: Send + Sync {
    /// The check's key, such as `disk.free`.
    fn key(&self) -> CheckKey;
    /// How often it runs unless settings override it.
    fn default_cadence(&self) -> SignedDuration;
    /// The class of its observations unless an observation overrides it.
    fn class(&self) -> SignalClass;
    /// Runs once. The scheduler calls this through [`run_bounded`].
    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError>;
}

/// Runs `check` once, giving up with [`CheckError::Timeout`] after [`RUN_LIMIT`] as
/// measured by `ctx.clock`.
pub async fn run_bounded(
    check: &dyn Check,
    ctx: &mut CheckContext<'_>,
) -> Result<CheckReport, CheckError> {
    let clock = ctx.clock;
    let now = clock.now();
    let deadline = now.checked_add(RUN_LIMIT).unwrap_or(now);
    tokio::select! {
        result = check.run(ctx) => result,
        () = clock.sleep_until(deadline) => Err(CheckError::Timeout),
    }
}
