//! The production [`ProcessExecutor`]: the only place in Domovoi that starts processes.

#[cfg(unix)]
mod capture;
mod env;
#[cfg(unix)]
mod program;
#[cfg(unix)]
mod spawn;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec, Redactor};

pub use env::{DEFAULT_HOME, DEFAULT_LANG, DEFAULT_PATH, default_base_env, is_allowed_env_key};

/// Program file names that are never run, whatever directory they are in (R4).
pub const SHELL_NAMES: &[&str] = &["sh", "bash", "dash", "zsh", "fish", "ash", "ksh", "busybox"];

/// How long a timed-out process group gets between `SIGTERM` and `SIGKILL`.
pub const TERM_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Runs one argv vector with limits.
///
/// # What it provides
///
/// - **No shell.** The program is executed directly. It must be an absolute path to an
///   existing regular file with an execute bit, and neither its path nor the file it
///   resolves to may be named like a shell ([`SHELL_NAMES`]). Arguments reach the program
///   verbatim.
/// - **A cleared environment.** Only the base environment (`PATH`, `LANG`, `HOME` by
///   default) plus `spec.env` entries whose keys match `^[A-Z][A-Z0-9_]*$` and do not
///   start with `LD_`. Base keys cannot be overridden by `spec.env`.
/// - **A fixed working directory:** `spec.cwd`, or `/`.
/// - **Its own process group.** The child leads a new group; on timeout the whole group
///   gets `SIGTERM`, then `SIGKILL` after [`TERM_GRACE`]. When the program exits, any
///   process still in its group is killed, so nothing outlives a run.
/// - **An optional user switch.** With `run_as`, the child runs with that user's uid and
///   primary gid. Only a root agent can switch to another user. When a root process
///   switches, the standard library clears supplementary groups (`setgroups(0)`) before
///   `setuid`, so the child keeps none of the agent's groups.
/// - **Capped output.** Stdout and stderr are each read up to `max_output_bytes`; hitting
///   either cap marks the output truncated and kills the group.
/// - **Redaction.** Both streams are decoded as lossy UTF-8 and passed through the
///   redactor. A secret cut in half by the cap is not recognised.
///
/// # What it does not provide
///
/// No namespaces, seccomp filter, cgroup limits, capability dropping beyond the uid
/// switch, or filesystem isolation: the program sees the host as its user can. A process
/// that leaves its group (`setsid`) escapes the group kill. Container-level sandboxing
/// is future work.
///
/// Only Unix is supported; elsewhere [`ProcessExecutor::run`] returns
/// [`ProcessError::Spawn`].
pub struct SandboxExecutor {
    redactor: Arc<dyn Redactor>,
    base_env: BTreeMap<String, String>,
}

impl SandboxExecutor {
    /// An executor with the default base environment.
    #[must_use]
    pub fn new(redactor: Arc<dyn Redactor>) -> Self {
        Self {
            redactor,
            base_env: default_base_env(),
        }
    }

    /// Replaces the base environment every process starts with.
    #[must_use]
    pub fn with_base_env(mut self, base_env: BTreeMap<String, String>) -> Self {
        self.base_env = base_env;
        self
    }

    /// The environment a process with `spec_env` would receive.
    #[must_use]
    pub fn environment_for(&self, spec_env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        env::build(&self.base_env, spec_env)
    }
}

#[async_trait]
impl ProcessExecutor for SandboxExecutor {
    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        #[cfg(unix)]
        {
            let program = program::check(&spec.program)?;
            let env = self.environment_for(&spec.env);
            let user = match &spec.run_as {
                Some(name) => spawn::resolve_user(name)?,
                None => None,
            };
            let started = std::time::Instant::now();
            let child = spawn::spawn(&program, &spec, &env, user)?;
            let captured = capture::drive(child, &spec).await?;
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            Ok(ProcessOutput {
                exit_code: captured.exit_code,
                signal: captured.signal,
                stdout: self
                    .redactor
                    .redact(&String::from_utf8_lossy(&captured.stdout)),
                stderr: self
                    .redactor
                    .redact(&String::from_utf8_lossy(&captured.stderr)),
                truncated: captured.truncated,
                timed_out: captured.timed_out,
                duration_ms,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (&self.redactor, spec);
            Err(ProcessError::Spawn(
                "process execution is only supported on Unix".to_owned(),
            ))
        }
    }
}
