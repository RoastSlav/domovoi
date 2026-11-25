//! The only place the helper spawns `nft` and `conntrack`.
//!
//! Every call goes through the injected `ProcessExecutor` with an absolute program found by
//! [`locate`], an argv vector, the fixed environment and a timeout. A script goes on standard
//! input; nothing here builds a shell command or interpolates text into an argument.

use std::collections::BTreeMap;

use domovoi_core::containment::templates::{NFT_ENV, script_args};
use domovoi_core::ports::{HostInspect, ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};
use jiff::SignedDuration;

use crate::reads::tools::{Tool, locate};

/// The cap on what a script's standard output and error may hold.
const SCRIPT_CAP: u32 = 64 * 1024;
/// The cap on a JSON read.
const LIST_CAP: u32 = 8 * 1024 * 1024;

/// What a finished call produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The exit code, `None` when a signal ended the process.
    pub exit: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl Run {
    /// Whether the call exited 0.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.exit == Some(0)
    }
}

/// Why a call produced no [`Run`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Fault {
    /// The program is not installed, or is not a sound root-owned file.
    #[error("the program is missing")]
    ToolMissing,
    /// The timeout killed the process.
    #[error("the call timed out")]
    TimedOut,
    /// The process could not be started.
    #[error("the call could not start: {0}")]
    Spawn(String),
}

/// Runs `nft` and `conntrack`.
#[derive(Clone, Copy)]
pub struct Runner<'a> {
    /// Runs the process.
    pub exec: &'a dyn ProcessExecutor,
    /// Vets the program.
    pub inspect: &'a dyn HostInspect,
    /// The longest one call may run.
    pub timeout: SignedDuration,
    /// A network namespace to enter first; only the self-test and the drills set it.
    pub netns: Option<&'a str>,
}

impl Runner<'_> {
    fn env() -> BTreeMap<String, String> {
        NFT_ENV
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    async fn run(
        &self,
        tool: Tool,
        args: Vec<String>,
        stdin: Option<String>,
        cap: u32,
    ) -> Result<Run, Fault> {
        let program = locate(self.inspect, tool).ok_or(Fault::ToolMissing)?;
        let (program, args) = match self.netns {
            None => (program.to_owned(), args),
            Some(ns) => {
                let ip = locate(self.inspect, Tool::Ip).ok_or(Fault::ToolMissing)?;
                let mut wrapped = vec![
                    "netns".to_owned(),
                    "exec".to_owned(),
                    ns.to_owned(),
                    program.to_owned(),
                ];
                wrapped.extend(args);
                (ip.to_owned(), wrapped)
            }
        };
        let spec = ProcessSpec {
            program,
            args,
            env: Self::env(),
            cwd: None,
            stdin,
            timeout: self.timeout,
            max_output_bytes: cap,
            run_as: None,
        };
        match self.exec.run(spec).await {
            Ok(ProcessOutput { timed_out: true, .. }) => Err(Fault::TimedOut),
            Ok(out) => Ok(Run {
                exit: out.exit_code,
                stdout: out.stdout,
                stderr: out.stderr,
            }),
            Err(ProcessError::NotFound | ProcessError::NotExecutable) => Err(Fault::ToolMissing),
            Err(err) => Err(Fault::Spawn(err.to_string())),
        }
    }

    /// Sends `script` to `nft -f -`.
    pub async fn script(&self, script: &str) -> Result<Run, Fault> {
        self.run(Tool::Nft, script_args(), Some(script.to_owned()), SCRIPT_CAP)
            .await
    }

    /// Runs `nft` with the read `args` of a template and returns its JSON.
    pub async fn read(&self, args: Vec<String>) -> Result<Run, Fault> {
        self.run(Tool::Nft, args, None, LIST_CAP).await
    }

    /// Runs `nft --version`.
    pub async fn version(&self) -> Result<Run, Fault> {
        self.run(Tool::Nft, vec!["--version".to_owned()], None, SCRIPT_CAP)
            .await
    }

    /// Runs `conntrack` with the delete `args` of a template.
    pub async fn conntrack(&self, args: Vec<String>) -> Result<Run, Fault> {
        self.run(Tool::Conntrack, args, None, SCRIPT_CAP).await
    }
}
