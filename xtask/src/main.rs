//! Developer tasks for the Domovoi workspace.
//!
//! - `cargo xtask arch-check [--base <ref>]`: enforce ARCHITECTURE.md rules.
//! - `cargo xtask ci [--fast] [--web]`: run the same gates CI runs.
//! - `cargo xtask openapi [--check]`: write (or check) `web/src/api/openapi.json`.
//! - `cargo xtask docs-config [--check]`: write (or check) the configuration reference.
//! - `cargo xtask docs-links`: check the book's links, anchors and pages.
//!
//! xtask is not part of the product; it may spawn processes (cargo, git, npm).

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::disallowed_methods)]

mod arch;
mod ci;
mod docs_config;
mod docs_links;
mod openapi;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Command-line interface for xtask.
#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Domovoi developer tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the architecture rules (dependency graph, forbidden APIs, manifests, sizes).
    ArchCheck {
        /// Git ref to compare against for change-based rules (append-only migrations, ADR required).
        #[arg(long)]
        base: Option<String>,
    },
    /// Run the local CI pipeline: fmt, clippy, tests, arch-check, deny.
    Ci {
        /// Skip cargo-deny and the web checks.
        #[arg(long)]
        fast: bool,
        /// Also run the web checks (npm ci, lint, typecheck, test, build).
        #[arg(long)]
        web: bool,
    },
    /// Write the hub's OpenAPI document to web/src/api/openapi.json.
    Openapi {
        /// Fail if the committed file differs instead of writing it.
        #[arg(long)]
        check: bool,
    },
    /// Write the configuration reference in the book from the configuration schemas.
    DocsConfig {
        /// Fail if the committed page differs instead of writing it.
        #[arg(long)]
        check: bool,
    },
    /// Check every link, anchor and page in the book.
    DocsLinks,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = workspace_root()?;
    match cli.command {
        Command::ArchCheck { base } => arch::run(&root, base.as_deref()),
        Command::Ci { fast, web } => ci::run(&root, fast, web),
        Command::Openapi { check } => openapi::run(&root, check),
        Command::DocsConfig { check } => docs_config::run(&root, check),
        Command::DocsLinks => docs_links::run(&root),
    }
}

fn workspace_root() -> anyhow::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("xtask must live one level below the workspace root"))
}
