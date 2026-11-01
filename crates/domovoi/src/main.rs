//! The `domovoi` binary: hub, agent and CLI tools in one executable.
//!
//! This is the outer composition root (ARCHITECTURE.md R6). It parses the command line,
//! initialises crypto, logging and metrics once, and hands each subcommand to library
//! code in `cmd/`. No business logic lives here.

#![allow(clippy::print_stdout)]

mod cmd;
mod init;
mod local_link;

use std::process::ExitCode;

use clap::Parser;

use crate::cmd::Command;
use crate::init::{LogFormat, Logging};

/// Command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "domovoi",
    version,
    about = "Self-hosted server overseer with AI triage and human-approved actions"
)]
struct Cli {
    /// Log output format.
    #[arg(long, global = true, value_enum, default_value_t = LogFormat::Human)]
    log_format: LogFormat,
    /// Log filter, such as `info` or `domovoi_hub=debug`. Overrides `RUST_LOG`.
    #[arg(long, global = true)]
    log_level: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Version);
    if let Err(err) = init::install_crypto() {
        return cmd::fail(&err);
    }
    init::logging(&Logging {
        format: cli.log_format,
        filter: cli.log_level,
        quiet: command.is_quiet(),
    });
    match cmd::run(command) {
        Ok(code) => code,
        Err(err) => cmd::fail(&err),
    }
}
