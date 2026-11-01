//! `domovoi digest now`.

use crate::ExitCode;
use std::sync::Arc;

use clap::Subcommand;
use domovoi_hub::admin::OfflineAdmin;

use super::HubConfigArg;
use crate::init::SystemClock;

/// Digest subcommands.
#[derive(Debug, Subcommand)]
pub enum DigestCommand {
    /// Build today's digest and send it to every notifier now.
    Now(HubConfigArg),
}

pub async fn run(command: DigestCommand) -> anyhow::Result<ExitCode> {
    let DigestCommand::Now(args) = command;
    let config = args.effective().await?;
    let admin = OfflineAdmin::open(config, Arc::new(SystemClock)).await?;
    let sent = admin.send_digest().await;
    admin.close().await;
    println!("Sent the digest for {}.", sent?);
    Ok(ExitCode::SUCCESS)
}
