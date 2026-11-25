//! The production [`RouteSource`]: `ip -j` run through the [`ProcessExecutor`] port.
//!
//! Every call is one argv vector with a complete environment of `LC_ALL=C`, a 5 second timeout
//! and a 4 MiB output cap. A process that cannot start, times out, writes more than the cap,
//! exits non-zero or prints something unparsable is a failed read, never a partial answer. The
//! one non-zero exit that is not a failure is a table dump for a table that does not exist: the
//! kernel prints `[]` and an error and exits 2, and a table that does not exist has no routes.

use core::net::IpAddr;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::attest::Family;
use domovoi_core::ports::{HostInspect, ProcessExecutor, ProcessOutput, ProcessSpec};
use jiff::SignedDuration;

use super::ip_json::{parse_addresses, parse_lookup, parse_routes, parse_rules};
use super::observe::{IfAddress, Lookup, PolicyRule, ReadError, RouteSource, TableRoute};

/// The places `ip` is looked for, in order; the first that exists is run.
pub const IP_PROGRAMS: [&str; 4] = ["/usr/sbin/ip", "/sbin/ip", "/usr/bin/ip", "/bin/ip"];
/// How long one `ip` call may run.
pub const READ_TIMEOUT: SignedDuration = SignedDuration::from_secs(5);
/// The most output of one `ip` call that is read.
pub const MAX_OUTPUT_BYTES: u32 = 4 * 1024 * 1024;

/// What the kernel prints when a routing table does not exist.
const NO_SUCH_TABLE: &str = "FIB table does not exist";

/// Why one `ip` call produced no usable output.
enum RunFailure {
    /// No `ip` program was found.
    NoProgram,
    /// The process could not run, ran out of time or space, or exited non-zero.
    Failed,
}

/// Reads routing state by running `ip`.
pub struct IpSource {
    exec: Arc<dyn ProcessExecutor>,
    inspect: Arc<dyn HostInspect>,
}

impl IpSource {
    /// A source that runs `ip` through `exec` and finds it through `inspect`.
    #[must_use]
    pub fn new(exec: Arc<dyn ProcessExecutor>, inspect: Arc<dyn HostInspect>) -> Self {
        Self { exec, inspect }
    }

    fn program(&self) -> Option<&'static str> {
        IP_PROGRAMS
            .iter()
            .copied()
            .find(|path| self.inspect.lstat(Path::new(path)).is_ok())
    }

    async fn run(&self, args: &[&str]) -> Result<ProcessOutput, RunFailure> {
        let program = self.program().ok_or(RunFailure::NoProgram)?;
        let spec = ProcessSpec {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: BTreeMap::from([("LC_ALL".to_owned(), "C".to_owned())]),
            cwd: None,
            stdin: None,
            timeout: READ_TIMEOUT,
            max_output_bytes: MAX_OUTPUT_BYTES,
            run_as: None,
        };
        let output = self.exec.run(spec).await.map_err(|_| RunFailure::Failed)?;
        if output.timed_out || output.truncated || output.signal.is_some() {
            return Err(RunFailure::Failed);
        }
        Ok(output)
    }

    /// The stdout of a call that exited 0.
    async fn read(&self, args: &[&str]) -> Result<String, RunFailure> {
        let output = self.run(args).await?;
        if output.exit_code == Some(0) {
            Ok(output.stdout)
        } else {
            Err(RunFailure::Failed)
        }
    }
}

fn family_flag(family: Family) -> &'static str {
    match family {
        Family::Inet => "-4",
        Family::Inet6 => "-6",
    }
}

#[async_trait]
impl RouteSource for IpSource {
    async fn rules(&self) -> Result<Vec<PolicyRule>, ReadError> {
        let mut rules = Vec::new();
        for family in [Family::Inet, Family::Inet6] {
            let args = ["-N", "-j", family_flag(family), "rule", "show"];
            let text = self
                .read(&args)
                .await
                .map_err(|_| ReadError::RulesUnreadable)?;
            rules.extend(parse_rules(&text, family)?);
        }
        Ok(rules)
    }

    async fn table_routes(&self, table: u32) -> Result<Vec<TableRoute>, ReadError> {
        let number = table.to_string();
        let mut routes = Vec::new();
        for family in [Family::Inet, Family::Inet6] {
            let args = [
                "-N",
                "-j",
                family_flag(family),
                "route",
                "show",
                "table",
                &number,
            ];
            let output = self
                .run(&args)
                .await
                .map_err(|_| ReadError::TableUnreadable(table))?;
            let absent = output.exit_code == Some(2)
                && output.stdout.trim() == "[]"
                && output.stderr.contains(NO_SUCH_TABLE);
            if absent {
                continue;
            }
            if output.exit_code != Some(0) {
                return Err(ReadError::TableUnreadable(table));
            }
            routes.extend(parse_routes(&output.stdout, family, table)?);
        }
        Ok(routes)
    }

    async fn addresses(&self) -> Result<Vec<IfAddress>, ReadError> {
        let text = self
            .read(&["-N", "-j", "addr", "show"])
            .await
            .map_err(|_| ReadError::AddressesUnreadable)?;
        parse_addresses(&text)
    }

    async fn lookup(&self, endpoint: IpAddr) -> Result<Lookup, ReadError> {
        let address = endpoint.to_string();
        let flag = family_flag(Family::of(endpoint));
        let text = self
            .read(&["-N", "-j", "-d", flag, "route", "get", &address])
            .await
            .map_err(|_| ReadError::LookupFailed)?;
        parse_lookup(&text, endpoint)
    }
}
