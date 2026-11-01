//! `domovoi selftest`.

use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use domovoi_hub::runtime::HubDeps;
use domovoi_hub::selftest::{self, Mode};
use domovoi_runner::SandboxExecutor;

use super::{HubConfigArg, load_hub, redactor};
use crate::init::SystemClock;

/// `selftest`.
#[derive(Debug, Args)]
pub struct SelftestArgs {
    #[command(flatten)]
    config: HubConfigArg,
    /// Use a built-in scripted model and a recording chat gateway instead of the
    /// configured ones: proves the pipeline without network access.
    #[arg(long)]
    offline: bool,
}

pub async fn run(args: SelftestArgs) -> anyhow::Result<ExitCode> {
    let config = load_hub(&args.config.config)?;
    let mode = if args.offline {
        Mode::Offline
    } else {
        Mode::Online
    };
    let executor = Arc::new(SandboxExecutor::new(redactor(&config)));
    let report = selftest::run(
        config,
        HubDeps {
            clock: Arc::new(SystemClock),
            process_executor: Some(executor),
            metrics: None,
        },
        mode,
    )
    .await;
    print!("{report}");
    if report.passed() {
        println!("selftest passed");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("selftest failed");
        Ok(ExitCode::FAILURE)
    }
}
