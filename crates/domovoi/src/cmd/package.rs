//! `domovoi package import` and `domovoi package list`.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand};
use domovoi_core::install::PackageArtifact;
use domovoi_hub::admin::{AdminError, OfflineAdmin};

use super::HubConfigArg;
use crate::ExitCode;
use crate::init::SystemClock;

/// Package subcommands.
#[derive(Debug, Subcommand)]
pub enum PackageCommand {
    /// Import a signed package archive and its signature file into the hub's store.
    Import(ImportArgs),
    /// List the imported packages.
    List(HubConfigArg),
}

/// `package import`.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// The `<name>-<version>.tar.zst` archive.
    archive: PathBuf,
    /// The detached `.sig.toml` signature file.
    signature: PathBuf,
    #[command(flatten)]
    config: HubConfigArg,
}

pub async fn run(command: PackageCommand) -> anyhow::Result<ExitCode> {
    match command {
        PackageCommand::Import(args) => import(args).await,
        PackageCommand::List(args) => list(args).await,
    }
}

/// Opens the data directory, mapping a running hub to a plain instruction.
async fn open(config: &HubConfigArg) -> anyhow::Result<OfflineAdmin> {
    let config = config.effective().await?;
    let data_dir = config.hub.data_dir.clone();
    OfflineAdmin::open(config, Arc::new(SystemClock))
        .await
        .map_err(|err| match err {
            AdminError::HubRunning(_) => anyhow::anyhow!(
                "a hub is running on {}; stop it first, or import through the web UI",
                data_dir.display()
            ),
            other => anyhow::anyhow!("{other}"),
        })
}

async fn import(args: ImportArgs) -> anyhow::Result<ExitCode> {
    let admin = open(&args.config).await?;
    let result = admin.package_import(&args.archive, &args.signature).await;
    admin.close().await;
    match result {
        Ok(outcome) => {
            let verb = if outcome.already_imported {
                "Already imported"
            } else {
                "Imported"
            };
            println!("{verb}:");
            print_artifact(&outcome.artifact);
            Ok(ExitCode::SUCCESS)
        }
        Err(AdminError::Package(err)) => {
            super::warn(&format!("refused: {}: {err}", err.code()));
            Ok(ExitCode::FAILURE)
        }
        Err(other) => Err(anyhow::anyhow!("{other}")),
    }
}

async fn list(args: HubConfigArg) -> anyhow::Result<ExitCode> {
    let admin = open(&args).await?;
    let artifacts = admin.package_list().await;
    admin.close().await;
    let artifacts = artifacts.map_err(|err| anyhow::anyhow!("{err}"))?;
    if artifacts.is_empty() {
        println!("No packages imported.");
    }
    for artifact in &artifacts {
        print_artifact(artifact);
    }
    Ok(ExitCode::SUCCESS)
}

fn print_artifact(artifact: &PackageArtifact) {
    println!("  package: {}", artifact.name);
    println!("  version: {}", artifact.version);
    println!("  digest:  {}", artifact.archive_sha256);
    println!("  key id:  {}", artifact.key_id);
    println!(
        "  label:   {}",
        artifact.signer_label.as_deref().unwrap_or("(none)")
    );
}
