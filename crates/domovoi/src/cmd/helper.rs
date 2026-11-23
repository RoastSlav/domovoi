//! `domovoi helper serve|init|repair`: the root helper's three entry points.
//!
//! `serve` is started by the socket-activated unit, one process per connection. `init` and
//! `repair` are owner commands run by hand as root. Nothing else may start a repair.

use crate::ExitCode;
use clap::Subcommand;

/// Helper subcommands.
#[derive(Debug, Subcommand)]
pub enum HelperCommand {
    /// Serve the one connection systemd passes on standard input, then exit.
    Serve,
    /// Create the helper state directory and its lock (root only; changes nothing that exists).
    Init,
    /// Put wrong modes and owners right, after the owner types `repair` (root only).
    Repair,
}

#[cfg(unix)]
pub async fn run(command: HelperCommand) -> anyhow::Result<ExitCode> {
    unix::run(command).await
}

#[cfg(not(unix))]
pub fn run(_command: HelperCommand) -> std::future::Ready<anyhow::Result<ExitCode>> {
    std::future::ready(Err(anyhow::anyhow!("the helper runs on Linux hosts only")))
}

#[cfg(unix)]
mod unix {
    use super::HelperCommand;
    use crate::ExitCode;
    use crate::init::SystemClock;
    use std::path::Path;
    use std::sync::Arc;

    use domovoi_config::{HelperConfig, load_helper};
    use domovoi_core::identity::HostDirs;
    use domovoi_core::ports::HostInspect;
    use domovoi_helper::adopt::adopt_stdin_connection;
    use domovoi_helper::inspect::UnixHostInspect;
    use domovoi_helper::repair::{
        RepairOutcome, TerminalConfirm, UnixRepairOps, render_plan, render_remaining, run_repair,
    };
    use domovoi_helper::serve::{ServeContext, ServeOutcome, serve_connection};
    use domovoi_helper::state::{InitOutcome, StateLayout, init};

    /// Where `helper.toml` is read from.
    const HELPER_TOML: &str = "/etc/domovoi/helper.toml";

    pub async fn run(command: HelperCommand) -> anyhow::Result<ExitCode> {
        let dirs = HostDirs::default();
        match command {
            HelperCommand::Serve => serve(dirs).await,
            HelperCommand::Init => init_state(&dirs),
            HelperCommand::Repair => repair(&dirs),
        }
    }

    /// `helper.toml` as it is, or the fresh-install default when there is no file. A file that
    /// does not parse is kept as its issues, so the start check can refuse with the token.
    fn load_config(path: &Path) -> Result<HelperConfig, Vec<domovoi_config::ConfigIssue>> {
        if !path.exists() {
            return Ok(HelperConfig::default());
        }
        load_helper(path).map(|(config, _warnings)| config)
    }

    async fn serve(dirs: HostDirs) -> anyhow::Result<ExitCode> {
        let conn = adopt_stdin_connection()?;
        let ctx = ServeContext::new(
            load_config(Path::new(HELPER_TOML)),
            Arc::new(UnixHostInspect::new()),
            Arc::new(SystemClock),
            dirs,
            env!("CARGO_PKG_VERSION"),
        );
        match serve_connection(conn, &ctx).await {
            ServeOutcome::Failed(why) => anyhow::bail!("serving the connection failed: {why}"),
            _ => Ok(ExitCode::SUCCESS),
        }
    }

    fn require_root() -> anyhow::Result<()> {
        anyhow::ensure!(
            UnixHostInspect::new().euid() == 0,
            "this command changes root-owned files; run it as root"
        );
        Ok(())
    }

    fn init_state(dirs: &HostDirs) -> anyhow::Result<ExitCode> {
        require_root()?;
        match init(StateLayout::PRODUCTION, &dirs.helper_dir) {
            Ok(InitOutcome::Created) => println!("created {}", dirs.helper_dir.display()),
            Ok(InitOutcome::AlreadyInitialised) => println!("already initialised"),
            Err(domovoi_helper::state::InitError::Faults(faults)) => {
                for fault in &faults {
                    crate::cmd::warn(domovoi_helper::selftest::render_fault(fault).trim_end());
                }
                crate::cmd::warn("nothing was changed; run `domovoi helper repair`");
                return Ok(ExitCode::FAILURE);
            }
            Err(err) => return Err(err.into()),
        }
        Ok(ExitCode::SUCCESS)
    }

    fn repair(dirs: &HostDirs) -> anyhow::Result<ExitCode> {
        let inspect = UnixHostInspect::new();
        let report = run_repair(&inspect, dirs, &TerminalConfirm, &UnixRepairOps)?;
        match &report.outcome {
            RepairOutcome::NotRoot => {
                crate::cmd::warn("this command changes root-owned files; run it as root");
                Ok(ExitCode::FAILURE)
            }
            RepairOutcome::NoTerminal(token) => {
                crate::cmd::warn(&format!("{}: run it from a terminal", token.as_str()));
                Ok(ExitCode::FAILURE)
            }
            RepairOutcome::NothingToDo => {
                println!("{}", render_plan(&report.plan).trim_end());
                print!("{}", render_remaining(&report.remaining));
                Ok(ExitCode::from(u8::from(!report.remaining.is_empty())))
            }
            RepairOutcome::Declined => {
                println!("nothing was changed");
                Ok(ExitCode::FAILURE)
            }
            RepairOutcome::Applied { changed, failed } => {
                println!("changed {} path(s)", changed.len());
                for (path, why) in failed {
                    crate::cmd::warn(&format!("{}: {why}", path.display()));
                }
                print!("{}", render_remaining(&report.remaining));
                Ok(ExitCode::from(u8::from(
                    !failed.is_empty() || !report.remaining.is_empty(),
                )))
            }
        }
    }
}
