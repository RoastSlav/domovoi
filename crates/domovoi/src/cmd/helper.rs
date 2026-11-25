//! `domovoi helper serve|init|repair|rotate-key|recover|fingerprint`: the root helper's entry
//! points.
//!
//! `serve` is started by the socket-activated unit, one process per connection. The others are
//! owner commands run by hand as root; `init` is also run by the installer. Nothing else may
//! start a repair, a rotation or a recovery.

use crate::ExitCode;
use clap::Subcommand;

/// Helper subcommands.
#[derive(Debug, Subcommand)]
pub enum HelperCommand {
    /// Serve the one connection systemd passes on standard input, then exit.
    Serve,
    /// Create the helper state directory, its lock, the attestation key and the first state
    /// (root only; never replaces a key or a state that exists).
    Init,
    /// Put wrong modes and owners right, after the owner types `repair` (root only).
    Repair,
    /// Replace the attestation key with a new one and retire the old public key (root only).
    /// The hub shows "helper key changed" until the owner enrols the new fingerprint.
    RotateKey,
    /// Start over from lost or damaged state: retire what can be read and make a new key (root
    /// only).
    Recover,
    /// Print the attestation key's fingerprint and key id, to compare with the hub (root only).
    Fingerprint,
    /// Test whether replacing a firewall element ever lets a packet through, in throwaway
    /// network namespaces (root only; minutes). The installer and a one-shot unit run it.
    NftSelftest {
        /// Exit at once when the stored result is for this kernel, `nft` version and layout.
        #[arg(long)]
        if_changed: bool,
        /// Test only the interval sets and keep the stored plain-set verdict.
        #[arg(long)]
        interval_only: bool,
    },
    /// The UDP sender the self-test starts inside its sender namespace. Not for people.
    #[command(hide = true)]
    NftSelftestSender {
        /// `--src`, `--dst`, `--port` and `--seconds`.
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Run the containment drills in a throwaway namespace and record the result (root only;
    /// minutes). Unattended containment is dispatched only after they pass.
    ContainmentDrill,
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

    use crate::cmd::helper_keys as keys;
    use crate::cmd::helper_nft as nft;
    use domovoi_config::{HelperConfig, load_helper};
    use domovoi_core::identity::HostDirs;
    use domovoi_helper::adopt::adopt_stdin_connection;
    use domovoi_helper::inspect::UnixHostInspect;
    use domovoi_helper::reads::docker::client::BollardDockerRead;
    use domovoi_helper::repair::{
        RepairOutcome, TerminalConfirm, UnixRepairOps, render_plan, render_remaining, run_repair,
    };
    use domovoi_helper::serve::{ServeContext, ServeOutcome, serve_connection};

    /// Where `helper.toml` is read from.
    const HELPER_TOML: &str = "/etc/domovoi/helper.toml";

    pub async fn run(command: HelperCommand) -> anyhow::Result<ExitCode> {
        let dirs = HostDirs::default();
        match command {
            HelperCommand::Serve => serve(dirs).await,
            HelperCommand::Init => keys::init_state(&dirs),
            HelperCommand::Repair => repair(&dirs),
            HelperCommand::RotateKey => keys::replace_key(&dirs, true),
            HelperCommand::Recover => keys::replace_key(&dirs, false),
            HelperCommand::Fingerprint => keys::print_fingerprint(&dirs),
            HelperCommand::NftSelftest {
                if_changed,
                interval_only,
            } => nft::selftest(dirs, if_changed, interval_only).await,
            HelperCommand::NftSelftestSender { args } => nft::sender(&args),
            HelperCommand::ContainmentDrill => nft::drill(dirs).await,
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
        let config = load_config(Path::new(HELPER_TOML));
        let endpoint = config
            .as_ref()
            .ok()
            .and_then(HelperConfig::docker_socket)
            .map(|path| format!("unix://{}", path.display()));
        let docker = BollardDockerRead::new(endpoint.as_deref());
        let ctx = ServeContext::new(
            config,
            Arc::new(UnixHostInspect::new()),
            Arc::new(SystemClock),
            dirs,
            env!("CARGO_PKG_VERSION"),
        )
        .with_process_executor(keys::route_reader())
        .with_docker_read(Arc::new(docker));
        match serve_connection(conn, &ctx).await {
            ServeOutcome::Failed(why) => anyhow::bail!("serving the connection failed: {why}"),
            _ => Ok(ExitCode::SUCCESS),
        }
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
