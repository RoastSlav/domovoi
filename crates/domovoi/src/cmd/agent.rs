//! `domovoi agent run|enroll`.

use crate::ExitCode;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use domovoi_agent::{AgentRuntime, EnrollEnv, EnrollToken, enroll, production};
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
            )?;
            let agent = AgentRuntime::new(config, deps).await?;
            agent.run(init::shutdown_on_signals()).await?;
            Ok(ExitCode::SUCCESS)
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
            println!(
                "Enrolled as host {} (agent {}). Start it with `domovoi agent run`.",
                identity.host_id, identity.agent_id
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}
