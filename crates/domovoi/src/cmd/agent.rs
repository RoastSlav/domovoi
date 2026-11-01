//! `domovoi agent run|enroll`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand};
use domovoi_agent::{AgentRuntime, EnrollEnv, EnrollToken, enroll, production};
use domovoi_runner::SandboxExecutor;
use url::Url;

use super::{DEFAULT_AGENT_CONFIG, load_agent, redactor};
use crate::init::{self, SystemClock};

/// Agent subcommands.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Run the agent until SIGINT or SIGTERM.
    Run(ConfigArg),
    /// Trade a one-time token for this agent's identity and pin the hub's certificate.
    Enroll(EnrollArgs),
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
    #[command(flatten)]
    config: ConfigArg,
}

pub async fn run(command: AgentCommand) -> anyhow::Result<ExitCode> {
    match command {
        AgentCommand::Run(args) => {
            let config = load_agent(&args.config)?;
            let redactor = redactor(&config);
            // INV-2: the runner's sandbox is the agent's only way to start a process.
            let executor = Arc::new(SandboxExecutor::new(redactor.clone()));
            let deps = production::deps(&config, executor, Arc::new(SystemClock), redactor)?;
            let agent = AgentRuntime::new(config, deps).await?;
            agent.run(init::shutdown_on_signals()).await?;
            Ok(ExitCode::SUCCESS)
        }
        AgentCommand::Enroll(args) => {
            let config = load_agent(&args.config.config)?;
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
