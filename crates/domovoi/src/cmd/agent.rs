//! `domovoi agent run|enroll`.

use crate::ExitCode;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use domovoi_agent::{
    AgentRuntime, EnrollEnv, EnrollToken, Journal, JournalError, enroll, production,
};
use domovoi_core::ports::Clock as _;
use domovoi_runner::SandboxExecutor;
use url::Url;

use super::{DEFAULT_AGENT_CONFIG, load_agent, redactor};
use crate::init::{self, SystemClock};
use crate::log_ring::LogRing;

/// Agent subcommands.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Run the agent until SIGINT or SIGTERM.
    Run(ConfigArg),
    /// Trade a one-time token for this agent's identity and pin the hub's certificate.
    Enroll(EnrollArgs),
    /// Show what the link last recorded: connected, reconnecting or incompatible.
    Status(ConfigArg),
    /// Check the three identities, the directories and the isolation on this host.
    Selftest(super::agent_selftest::SelftestArgs),
    /// Create, inspect or reset the durable action journal.
    #[command(subcommand)]
    Journal(JournalCommand),
}

/// `agent journal` subcommands. Both are local owner actions and never reachable from the hub.
#[derive(Debug, Subcommand)]
pub enum JournalCommand {
    /// Create the journal for the first time. Refuses if either journal file exists.
    Init(ConfigArg),
    /// Archive the existing journal and start a new epoch. Refuses while the agent owns it.
    Reset(ConfigArg),
}

/// `--config` for agent commands.
#[derive(Debug, Args)]
pub struct ConfigArg {
    /// The agent configuration file.
    #[arg(long, short, env = "DOMOVOI_AGENT_CONFIG", default_value = DEFAULT_AGENT_CONFIG)]
    config: PathBuf,
}

/// `agent enroll`.
#[derive(Debug, Args)]
pub struct EnrollArgs {
    /// The hub's agent server, such as `wss://hub.lan:7421`.
    #[arg(long)]
    hub: Url,
    /// The token from `domovoi hub token create`.
    #[arg(long, env = "DOMOVOI_ENROLL_TOKEN", hide_env_values = true)]
    token: String,
    /// The hub certificate's SHA-256, as printed with the token.
    #[arg(long)]
    fingerprint: String,
    /// This host's name in Domovoi, as printed with the token. Used to write
    /// `agent.toml` when it does not exist yet. An existing file keeps everything except
    /// its `hub_url`, which becomes `--hub` when they differ.
    #[arg(long)]
    name: Option<String>,
    #[command(flatten)]
    config: ConfigArg,
}

pub async fn run(command: AgentCommand, logs: Arc<LogRing>) -> anyhow::Result<ExitCode> {
    match command {
        AgentCommand::Run(args) => {
            let config = load_agent(&args.config)?;
            let redactor = redactor(&config);
            // Scrub this process's log lines with this agent's secrets as they come in.
            logs.set_redactor(redactor.clone());
            // INV-2: the runner's sandbox is the agent's only way to start a process.
            let executor = Arc::new(SandboxExecutor::new(redactor.clone()));
            let deps = production::deps(
                &config,
                executor,
                Arc::new(SystemClock),
                redactor,
                Some(logs),
            )?
            .with_boundary_in_effect(boundary_in_effect());
            let agent = AgentRuntime::new(config, deps).await?;
            agent.run(init::shutdown_on_signals()).await?;
            Ok(ExitCode::SUCCESS)
        }
        AgentCommand::Selftest(args) => super::agent_selftest::run(args).await,
        AgentCommand::Journal(command) => {
            let (args, reset) = match command {
                JournalCommand::Init(args) => (args, false),
                JournalCommand::Reset(args) => (args, true),
            };
            let config = load_agent(&args.config)?;
            let clock = SystemClock;
            let fs = production::host_fs(&config);
            let snapshot = domovoi_agent::boot_snapshot(&*fs).await?;
            let boot_id = snapshot.boot_id.to_string();
            let dir = &config.agent.state_dir;
            let result = if reset {
                Journal::reset(dir, clock.now(), &boot_id, snapshot.monotonic_ns)
            } else {
                Journal::init(dir, clock.now(), &boot_id, snapshot.monotonic_ns)
            };
            match result {
                Ok(epoch) => {
                    println!(
                        "The action journal at {} now uses epoch {epoch}.",
                        dir.display()
                    );
                    hand_over_state(dir)?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(JournalError::Locked(_)) => {
                    super::warn(
                        "the agent owns the journal; stop it with `systemctl stop domovoi-agent`, \
                         run this command, then start it again",
                    );
                    Ok(ExitCode::FAILURE)
                }
                Err(err) => Err(err.into()),
            }
        }
        AgentCommand::Status(args) => {
            let config = load_agent(&args.config)?;
            match domovoi_agent::link::status::read(&config.agent.state_dir)? {
                None => println!("no status recorded; the agent has not run yet."),
                Some(status) if status.state == "incompatible" => {
                    match (
                        status.agent_protocol.as_deref(),
                        status.hub_protocol.as_deref(),
                    ) {
                        (Some(agent), Some(hub)) => {
                            println!(
                                "incompatible: this agent speaks protocol {agent}, the hub \
                                 speaks {hub}. Upgrade the agent, then restart it. (updated {})",
                                status.updated_at
                            );
                            if let Some(detail) = status.detail {
                                println!("{detail}");
                            }
                        }
                        _ => {
                            // A legacy `agent_too_old` reject names no protocol pair; the
                            // stored detail says what happened.
                            let detail = status
                                .detail
                                .unwrap_or_else(|| "upgrade the agent, then restart it".to_owned());
                            println!("incompatible: {detail} (updated {})", status.updated_at);
                        }
                    }
                }
                Some(status) if status.state == "connected" => {
                    let since = status.connected_since.unwrap_or(status.updated_at);
                    println!("connected since {since}.");
                }
                Some(status) => {
                    let detail = status.detail.unwrap_or_else(|| "not connected".to_owned());
                    println!("{}: {detail} (updated {})", status.state, status.updated_at);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        AgentCommand::Enroll(args) => {
            let path = &args.config.config;
            if !path.exists() {
                let name = args.name.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "{} does not exist; pass --name (as printed with the token) to write it",
                        path.display()
                    )
                })?;
                domovoi_agent::config_file::write_if_absent(
                    path,
                    name,
                    &args.hub,
                    &args.fingerprint,
                )?;
                println!("Wrote {}.", path.display());
            } else {
                let config = load_agent(path)?;
                if config.agent.hub_url != args.hub
                    && domovoi_agent::config_file::set_hub_url(path, &args.hub)?
                {
                    println!(
                        "Changed the hub address in {} to {}.",
                        path.display(),
                        args.hub
                    );
                }
            }
            let config = load_agent(path)?;
            let fingerprint = production::parse_fingerprint(&args.fingerprint)
                .map_err(|err| anyhow::anyhow!("--fingerprint: {err}"))?;
            let clock = SystemClock;
            let fs = production::host_fs(&config);
            let identity = enroll(
                &args.hub,
                EnrollToken::new(args.token.trim()),
                Some(fingerprint),
                &config,
                &config.agent.state_dir,
                EnrollEnv {
                    clock: &clock,
                    fs: &*fs,
                },
            )
            .await?;
            hand_over_state(&config.agent.state_dir)?;
            println!(
                "Enrolled as host {} (agent {}). Start it with `domovoi agent run`.",
                identity.host_id, identity.agent_id
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Whether this process is the `domovoi-agent` account with exactly the expected groups,
/// looked at once at start (ST-01, ST-02).
#[cfg(unix)]
fn boundary_in_effect() -> bool {
    domovoi_core::isolation::inspect_boundary(&domovoi_helper::inspect::UnixHostInspect::new())
        .in_effect()
}

#[cfg(not(unix))]
fn boundary_in_effect() -> bool {
    false
}

/// After an owner command that ran as root, gives the agent account what it wrote in `dir`,
/// so the unprivileged agent can read it. Does nothing when not root or when the account is
/// missing (a legacy install).
fn hand_over_state(dir: &std::path::Path) -> anyhow::Result<()> {
    if let Some(owner) = domovoi_agent::state_owner::resolve_agent_owner()? {
        let report = domovoi_agent::state_owner::hand_over(dir, owner)?;
        println!(
            "Gave {} of {} entries in {} to the agent account.",
            report.changed,
            report.visited,
            dir.display()
        );
    }
    Ok(())
}
