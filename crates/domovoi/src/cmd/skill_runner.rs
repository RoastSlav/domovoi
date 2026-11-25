//! `domovoi skill-runner serve`: the socket-activated skill runner, one process per connection.
//!
//! The unit passes the connection on standard input and both roots as arguments. The process
//! loads no configuration file: it serves one request, then exits 0. Only a usage error or a
//! defect exits non-zero.

use crate::ExitCode;
use clap::Subcommand;
use std::path::PathBuf;

/// Skill-runner subcommands.
#[derive(Debug, Subcommand)]
pub enum SkillRunnerCommand {
    /// Serve the one connection systemd passes on standard input, then exit.
    Serve {
        /// The skills directory the unit binds read-only (absolute).
        #[arg(long)]
        skills_dir: PathBuf,
        /// The packages directory the unit binds read-only (absolute).
        #[arg(long)]
        packages_dir: PathBuf,
    },
}

#[cfg(unix)]
pub async fn run(command: SkillRunnerCommand) -> anyhow::Result<ExitCode> {
    unix::run(command).await
}

#[cfg(not(unix))]
pub fn run(_command: SkillRunnerCommand) -> std::future::Ready<anyhow::Result<ExitCode>> {
    std::future::ready(Err(anyhow::anyhow!(
        "the skill runner runs on Linux hosts only"
    )))
}

#[cfg(unix)]
mod unix {
    use super::SkillRunnerCommand;
    use crate::ExitCode;
    use std::sync::Arc;

    use domovoi_core::isolation::IsolationReport;
    use domovoi_helper::adopt::adopt_stdin_connection;
    use domovoi_helper::inspect::UnixHostInspect;
    use domovoi_helper::probe::{ProbeArgs, collect};
    use domovoi_runner::serve::{ProbeCollector, ServeContext, serve_connection};

    /// The pid probed for point 6 when the caller names no helper: init, which must be hidden too.
    const FALLBACK_PID: u32 = 1;

    /// Collects the isolation observations through the host adapter.
    struct HostProbe(UnixHostInspect);

    impl ProbeCollector for HostProbe {
        fn collect(&self, helper_pid: Option<u32>) -> IsolationReport {
            collect(
                &self.0,
                &ProbeArgs {
                    helper_pid: helper_pid.unwrap_or(FALLBACK_PID),
                },
            )
        }
    }

    pub async fn run(command: SkillRunnerCommand) -> anyhow::Result<ExitCode> {
        let SkillRunnerCommand::Serve {
            skills_dir,
            packages_dir,
        } = command;
        for (name, dir) in [
            ("--skills-dir", &skills_dir),
            ("--packages-dir", &packages_dir),
        ] {
            if !dir.is_absolute() || !dir.exists() {
                crate::cmd::warn(&format!("{name} must be an existing absolute path"));
                return Ok(ExitCode::from(2));
            }
        }
        let stream = adopt_stdin_connection()?;
        let ctx = ServeContext::new(
            skills_dir,
            packages_dir,
            Arc::new(UnixHostInspect::new()),
            Arc::new(HostProbe(UnixHostInspect::new())),
        );
        serve_connection(stream, ctx).await?;
        Ok(ExitCode::SUCCESS)
    }
}
