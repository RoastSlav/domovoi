//! The `domovoi` binary: hub, agent and CLI tools in one executable.
//!
//! This is the outer composition root (ARCHITECTURE.md R6). It parses the
//! command line, selects the role and wires the hub and agent runtimes.

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

/// Top-level subcommands. Each build-plan prompt adds the ones it owns.
#[derive(Debug, Subcommand)]
enum Command {
    /// Print version and build information.
    Version,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Version) | None => {
            println!("domovoi {}", env!("CARGO_PKG_VERSION"));
        }
    }
    Ok(())
}
