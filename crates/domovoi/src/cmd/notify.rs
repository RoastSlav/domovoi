//! `domovoi notify test`.

use crate::ExitCode;
use std::sync::Arc;

use clap::Subcommand;
use domovoi_hub::admin::notify_test;

use super::{HubConfigArg, load_hub};
use crate::init::SystemClock;

/// Notification subcommands.
#[derive(Debug, Subcommand)]
pub enum NotifyCommand {
    /// Send a test card to each chat gateway and a test message to each notifier.
    Test(HubConfigArg),
}

pub async fn run(command: NotifyCommand) -> anyhow::Result<ExitCode> {
    let NotifyCommand::Test(args) = command;
    let config = load_hub(&args.config)?;
    let outcomes = notify_test(&config, Arc::new(SystemClock)).await?;
    if outcomes.is_empty() {
        println!("Nothing is configured to notify: enable [discord] or set a webhook_url.");
        return Ok(ExitCode::FAILURE);
    }
    let mut failed = false;
    for outcome in outcomes {
        match outcome.result {
            Ok(()) => println!("PASS  {}", outcome.target),
            Err(err) => {
                failed = true;
                println!("FAIL  {}: {err}", outcome.target);
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
