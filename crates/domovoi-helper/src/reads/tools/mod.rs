//! The three programs the helper runs for reads: `smartctl`, `dmesg` and `ss`.
//!
//! Each is found in a fixed list of absolute paths, checked on disk through `HostInspect`, and run
//! through the injected `ProcessExecutor` with a fixed argv, an empty environment and no shell.
//! The argv builders are pure functions of the tool path and the typed slots.

pub mod kernel;
pub mod net;
pub mod smart;
pub mod smart_glob;

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::helper::read::{
    DeviceRef, ReadFailure, ReadToken, TOOL_OUTPUT_BYTES, TOOL_TIMEOUT_SECS,
};
use domovoi_core::identity::PathKind;
use domovoi_core::ports::{
    HostInspect, InspectError, ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec,
};
use jiff::SignedDuration;

/// A program the helper may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `smartctl`, for `smart.read`.
    Smartctl,
    /// `dmesg`, for `kernel.read`.
    Dmesg,
    /// `ss`, for `net.read`.
    Ss,
    /// `busctl`, for `systemd.read` and the reads a unit restart makes.
    Busctl,
    /// `systemctl`, run only as `restart -- <Id>`.
    Systemctl,
    /// `nft`, run only with the fixed containment templates.
    Nft,
    /// `conntrack`, run only as `-D -s <source>` for a containment target.
    Conntrack,
    /// `ip`, run by the leak self-test to build and enter its throwaway namespace.
    Ip,
}

impl Tool {
    /// The absolute paths tried, in order. `PATH` is never consulted.
    #[must_use]
    pub const fn candidates(self) -> &'static [&'static str] {
        match self {
            Self::Smartctl => &["/usr/sbin/smartctl", "/usr/bin/smartctl", "/sbin/smartctl"],
            Self::Dmesg => &["/usr/bin/dmesg", "/bin/dmesg"],
            Self::Ss => &["/usr/bin/ss", "/bin/ss", "/usr/sbin/ss"],
            Self::Busctl => &["/usr/bin/busctl", "/bin/busctl"],
            Self::Systemctl => &["/usr/bin/systemctl", "/bin/systemctl"],
            Self::Nft => &["/usr/sbin/nft", "/usr/bin/nft", "/sbin/nft", "/bin/nft"],
            Self::Conntrack => &[
                "/usr/sbin/conntrack",
                "/usr/bin/conntrack",
                "/sbin/conntrack",
                "/bin/conntrack",
            ],
            Self::Ip => &["/usr/sbin/ip", "/usr/bin/ip", "/sbin/ip", "/bin/ip"],
        }
    }
}

/// The first candidate that exists, if it is a regular file owned by root that no one but root
/// may write. A candidate that exists and fails that test is not skipped: the answer is `None`.
#[must_use]
pub fn locate(inspect: &dyn HostInspect, tool: Tool) -> Option<&'static str> {
    for candidate in tool.candidates() {
        match inspect.lstat(Path::new(candidate)) {
            Ok(facts) => {
                let sound = facts.kind == Some(PathKind::File)
                    && !facts.symlink
                    && facts.uid == 0
                    && facts.mode & 0o022 == 0;
                return sound.then_some(*candidate);
            }
            Err(InspectError::NotFound) => {}
            Err(_) => return None,
        }
    }
    None
}

/// The arguments of `smartctl` for a scan.
#[must_use]
pub fn smart_scan_args() -> Vec<String> {
    to_strings(&["--scan-open", "--json"])
}

/// The arguments of `smartctl` for one device's report. The device starts with `/dev/`, so it
/// cannot be read as an option.
#[must_use]
pub fn smart_report_args(device: &DeviceRef) -> Vec<String> {
    let mut args = to_strings(&["--all", "--json", "--nocheck=standby"]);
    args.push(device.as_str().to_owned());
    args
}

/// The arguments of `dmesg`.
#[must_use]
pub fn kernel_args() -> Vec<String> {
    to_strings(&["--kernel", "--time-format=iso"])
}

/// The arguments of `ss`.
#[must_use]
pub fn net_args() -> Vec<String> {
    to_strings(&[
        "--no-header",
        "--listening",
        "--numeric",
        "--tcp",
        "--udp",
        "--processes",
    ])
}

fn to_strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// The invocation of `program` with `args`: empty environment, no working directory, no stdin,
/// run as the helper itself, the tool timeout and output cap.
#[must_use]
pub fn spec(program: &str, args: Vec<String>) -> ProcessSpec {
    ProcessSpec {
        program: program.to_owned(),
        args,
        env: BTreeMap::new(),
        cwd: None,
        stdin: None,
        timeout: SignedDuration::from_secs(i64::try_from(TOOL_TIMEOUT_SECS).unwrap_or(i64::MAX)),
        max_output_bytes: TOOL_OUTPUT_BYTES,
        run_as: None,
    }
}

/// What a tool needs to run: how to look at the host and how to start a process.
#[derive(Clone, Copy)]
pub struct ToolEnv<'a> {
    /// Finds and vets the program.
    pub inspect: &'a dyn HostInspect,
    /// Runs it.
    pub exec: &'a dyn ProcessExecutor,
}

/// Finds `tool`, runs it with `args` and returns what it printed. A missing or unsound program,
/// a spawn error and a timeout are failures; a non-zero exit is not, the caller decides.
pub async fn run(
    env: ToolEnv<'_>,
    tool: Tool,
    args: Vec<String>,
) -> Result<ProcessOutput, ReadFailure> {
    let program = locate(env.inspect, tool).ok_or(ReadFailure::bare(ReadToken::ToolMissing))?;
    match env.exec.run(spec(program, args)).await {
        Ok(output) if output.timed_out => Err(ReadFailure::bare(ReadToken::TimedOut)),
        Ok(output) => Ok(output),
        Err(ProcessError::NotFound | ProcessError::NotExecutable) => {
            Err(ReadFailure::bare(ReadToken::ToolMissing))
        }
        Err(err) => Err(ReadFailure::with_detail(
            ReadToken::CommandFailed,
            &err.to_string(),
        )),
    }
}

/// `command_failed` carrying the first line of what the tool said on stderr.
#[must_use]
pub fn command_failed(output: &ProcessOutput) -> ReadFailure {
    ReadFailure::with_detail(ReadToken::CommandFailed, &output.stderr)
}
