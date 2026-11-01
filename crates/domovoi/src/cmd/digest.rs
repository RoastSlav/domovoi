//! `domovoi digest now`.

use std::process::ExitCode;
use std::sync::Arc;

use clap::Subcommand;
use domovoi_hub::admin::OfflineAdmin;

use super::{HubConfigArg, load_hub};
use crate::init::SystemClock;

/// Digest subcommands.
#[derive(Debug, Subcommand)]
pub enum DigestCommand {
    /// Build today's digest and send it to every notifier now.
    Now(HubConfigArg),
}

pub async fn run(command: DigestCommand) -> anyhow::Result<ExitCode> {
    let DigestCommand::Now(args) = command;
    let config = load_hub(&args.config)?;
    let admin = OfflineAdmin::open(config, Arc::new(SystemClock)).await?;
    let sent = admin.send_digest().await;
    admin.close().await;
    println!("Sent the digest for {}.", sent?);
    Ok(ExitCode::SUCCESS)
}
