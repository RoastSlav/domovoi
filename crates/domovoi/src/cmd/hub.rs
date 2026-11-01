//! `domovoi hub run|token|agent|set-password`.

use crate::ExitCode;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use domovoi_core::ports::{Clock, ProcessExecutor};
use domovoi_hub::admin::OfflineAdmin;
use domovoi_hub::runtime::{HubDeps, HubRuntime};
use domovoi_runner::SandboxExecutor;

use super::{HubConfigArg, load_hub, redactor, warn};
use crate::init::{self, SystemClock};
use crate::local_link;

/// Hub subcommands.
#[derive(Debug, Subcommand)]
pub enum HubCommand {
    /// Run the hub until SIGINT or SIGTERM.
    Run(RunArgs),
    /// Enrollment tokens.
    #[command(subcommand)]
    Token(TokenCommand),
    /// Enrolled agents.
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Set the web UI admin password; creates the admin account on a new hub.
    SetPassword(SetPasswordArgs),
}

/// `hub run`.
#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    config: HubConfigArg,
    /// Also run an agent for this machine in the same process (single-box mode).
    #[arg(long, value_name = "AGENT_TOML")]
    with_local_agent: Option<PathBuf>,
}

/// `hub token`.
#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Create a single-use enrollment token for a host and print the enroll command.
    Create {
        /// The host's name, as it will appear in Domovoi.
        host: String,
        /// How long the token stays valid.
        #[arg(long, default_value = "15m")]
        ttl: domovoi_config::Duration,
        #[command(flatten)]
        config: HubConfigArg,
    },
}

/// `hub agent`.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Every host and its enrolled agent.
    List {
        #[command(flatten)]
        config: HubConfigArg,
    },
    /// Revoke a host's agent; it can never connect again until re-enrolled.
    Revoke {
        /// The host's name.
        host: String,
        #[command(flatten)]
        config: HubConfigArg,
    },
}

/// `hub set-password`.
#[derive(Debug, Args)]
pub struct SetPasswordArgs {
    #[command(flatten)]
    config: HubConfigArg,
    /// The account to change; by default the first (admin) account.
    #[arg(long)]
    username: Option<String>,
    /// Read the password from the first line of standard input instead of prompting.
    #[arg(long)]
    password_stdin: bool,
}

pub async fn run(command: HubCommand) -> anyhow::Result<ExitCode> {
    match command {
        HubCommand::Run(args) => run_hub(args).await,
        HubCommand::Token(TokenCommand::Create { host, ttl, config }) => {
            let admin = open(&config).await?;
            let invite = admin.create_token(&host, ttl.as_signed()).await;
            admin.close().await;
            let invite = invite?;
            println!(
                "Enrollment token for {host}, valid until {}.",
                invite.expires_at
            );
            println!("Hub fingerprint: {}", invite.fingerprint);
            println!("Run this on {host}:");
            println!();
            println!("  {}", invite.command);
            Ok(ExitCode::SUCCESS)
        }
        HubCommand::Agent(AgentCommand::List { config }) => {
            let admin = open(&config).await?;
            let rows = admin.agents().await;
            admin.close().await;
            let rows = rows?;
            println!("{:<24} {:<9} {:<12} ENROLLED", "HOST", "KIND", "VERSION");
            for row in rows {
                println!(
                    "{:<24} {:<9} {:<12} {}",
                    row.host,
                    row.kind,
                    row.version.as_deref().unwrap_or("-"),
                    row.enrolled_at
                        .map_or_else(|| "-".to_owned(), |t| t.to_string())
                );
            }
            println!("(the hub is not running, so no agent is connected)");
            Ok(ExitCode::SUCCESS)
        }
        HubCommand::Agent(AgentCommand::Revoke { host, config }) => {
            let admin = open(&config).await?;
            let revoked = admin.revoke(&host).await;
            admin.close().await;
            if revoked? {
                println!("Revoked the agent of {host}.");
            } else {
                println!("{host} has no agent to revoke.");
            }
            Ok(ExitCode::SUCCESS)
        }
        HubCommand::SetPassword(args) => set_password(args).await,
    }
}

async fn open(config: &HubConfigArg) -> anyhow::Result<OfflineAdmin> {
    let config = load_hub(&config.config)?;
    Ok(OfflineAdmin::open(config, Arc::new(SystemClock)).await?)
}

async fn set_password(args: SetPasswordArgs) -> anyhow::Result<ExitCode> {
    let password = if args.password_stdin {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim_end_matches(['\r', '\n']).to_owned()
    } else {
        let first = rpassword::prompt_password("New password: ")?;
        let again = rpassword::prompt_password("Repeat it: ")?;
        if first != again {
            anyhow::bail!("the passwords do not match");
        }
        first
    };
    let admin = open(&args.config).await?;
    let result = admin
        .set_password(args.username.as_deref(), &password)
        .await;
    admin.close().await;
    println!("Password set for {}.", result?);
    Ok(ExitCode::SUCCESS)
}

async fn run_hub(args: RunArgs) -> anyhow::Result<ExitCode> {
    let config = load_hub(&args.config.config)?;
    let local_agent = args
        .with_local_agent
        .as_deref()
        .map(super::load_agent)
        .transpose()?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let executor: Arc<dyn ProcessExecutor> = Arc::new(SandboxExecutor::new(redactor(&config)));
    let runtime = HubRuntime::build(
        config,
        HubDeps {
            clock: clock.clone(),
            process_executor: Some(executor.clone()),
            metrics: Some(init::metrics()?),
            apply: None,
        },
    )
    .await?;
    if let Some(link) = runtime.setup_link().await? {
        println!("No account exists yet. Finish setup in the web UI within 24 hours:");
        println!("  {link}");
        println!("(or run `domovoi hub set-password` while the hub is stopped)");
    }
    let shutdown = init::shutdown_on_signals();
    let agent = local_agent.map(|config| {
        tokio::spawn(local_link::run_agent(
            runtime.local_agents(),
            config,
            executor,
            clock,
            shutdown.clone(),
        ))
    });
    let result = runtime.run(shutdown.clone()).await;
    shutdown.cancel();
    if let Some(agent) = agent {
        match agent.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => warn(&format!("the local agent stopped: {err:#}")),
            Err(err) => warn(&format!("the local agent task failed: {err}")),
        }
    }
    result?;
    Ok(ExitCode::SUCCESS)
}
