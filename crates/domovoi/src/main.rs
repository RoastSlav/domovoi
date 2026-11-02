//! The `domovoi` developer binary and eventual product command line.
//!
//! The runtime composition roots are implemented in the hub and agent libraries. This
//! binary currently exposes version information and the hidden OpenAPI generator hook;
//! user-facing hub and agent commands are still planned.

#![allow(clippy::print_stdout)]

use clap::{Parser, Subcommand};

/// Command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "domovoi",
    version,
    about = "Self-hosted server overseer with AI triage and human-approved actions"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Print version and build information.
    Version,
    /// Print the hub's OpenAPI document as JSON (used by `cargo xtask openapi`).
    #[command(hide = true)]
    Openapi,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Version) | None => {
            println!("domovoi {}", env!("CARGO_PKG_VERSION"));
        }
        Some(Command::Openapi) => print!("{}", domovoi_hub::router::openapi_json()),
    }
    Ok(())
}
