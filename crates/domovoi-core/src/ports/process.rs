//! Running a program with limits (ADR 0004).

use std::collections::BTreeMap;

use async_trait::async_trait;
use jiff::SignedDuration;

/// One program invocation as an argv vector. There is no shell anywhere (R4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// Absolute path of the program.
    pub program: String,
    /// Arguments, passed as-is without any shell interpretation.
    pub args: Vec<String>,
    /// The complete environment. Adapters start from an empty environment and set only this.
    pub env: BTreeMap<String, String>,
    /// Working directory, or the adapter's default when `None`.
    pub cwd: Option<String>,
    /// Text written to standard input before it is closed.
    pub stdin: Option<String>,
    /// Wall-clock limit; the process group is killed when it elapses.
    pub timeout: SignedDuration,
    /// Cap on captured stdout and stderr, each.
    pub max_output_bytes: u32,
    /// Unix user to run as, or the current user when `None`.
    pub run_as: Option<String>,
}

/// What a finished (or killed) process produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessOutput {
    /// Exit code, or `None` when the process was killed by a signal.
    pub exit_code: Option<i32>,
    /// The signal that ended the process, if any.
    pub signal: Option<i32>,
    /// Captured standard output, cut at the output cap.
    pub stdout: String,
    /// Captured standard error, cut at the output cap.
    pub stderr: String,
    /// Whether either stream was cut.
    pub truncated: bool,
    /// Whether the timeout killed the process.
    pub timed_out: bool,
    /// Wall-clock run time in milliseconds.
    pub duration_ms: u64,
}

/// Why a process could not be run. A process that ran and failed is not an error; see
/// [`ProcessOutput::exit_code`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProcessError {
    /// The program does not exist.
    #[error("program not found")]
    NotFound,
    /// The program exists but is not executable.
    #[error("program not executable")]
    NotExecutable,
    /// The program is a shell; shells are never run (R4).
    #[error("refusing to run a shell")]
    ShellRefused,
    /// Switching to `run_as` failed.
    #[error("cannot switch user: {0}")]
    UserSwitch(String),
    /// Spawning failed for another reason.
    #[error("spawn failed: {0}")]
    Spawn(String),
}

/// Runs one argv with a timeout and output caps. Only `domovoi-runner` implements it (R3).
///
/// Adapters must refuse a relative `program` with [`ProcessError::NotFound`] and any shell
/// (`sh`, `bash`, `dash`, `zsh`, `fish`, `ash`, `ksh`) with [`ProcessError::ShellRefused`].
#[async_trait]
pub trait ProcessExecutor: Send + Sync {
    /// Runs `spec` to completion or until its timeout.
    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError>;
}
