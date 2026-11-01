//! `domovoi hub run|token|agent|set-password`.

use crate::ExitCode;
use std::io::BufRead;
use std::sync::Arc;

use clap::{Args, Subcommand};
use domovoi_hub::admin::OfflineAdmin;

use super::HubConfigArg;
use crate::init::SystemClock;

/// Hub subcommands.
#[derive(Debug, Subcommand)]
pub enum HubCommand {
    /// Run the hub until SIGINT or SIGTERM.
    Run(super::hub_run::RunArgs),
    /// Enrollment tokens.
    #[command(subcommand)]
    Token(TokenCommand),
    /// Enrolled agents.
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Set the web UI admin password; creates the admin account on a new hub.
    SetPassword(SetPasswordArgs),
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
        HubCommand::Run(args) => super::hub_run::run(args).await,
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
    let config = config.effective().await?;
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
