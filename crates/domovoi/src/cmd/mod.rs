//! One module per subcommand. Each parses nothing itself and calls library APIs.

mod agent;
mod check;
mod digest;
mod hub;
mod hub_run;
mod notify;
mod selftest;
mod version;

use crate::ExitCode;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Subcommand;
use domovoi_config::{AgentConfig, ConfigIssue, HubConfig, Secrets, SystemEnv};
use domovoi_core::ports::Redactor;
use domovoi_hub::PatternRedactor;
use domovoi_hub::settings::ConfigSources;

/// Where `hub.toml` is read from unless `--config` says otherwise.
pub const DEFAULT_HUB_CONFIG: &str = "/etc/domovoi/hub.toml";
/// Where `agent.toml` is read from unless `--config` says otherwise.
pub const DEFAULT_AGENT_CONFIG: &str = "/etc/domovoi/agent.toml";

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run and administer the hub.
    #[command(subcommand)]
    Hub(hub::HubCommand),
    /// Run or enroll the agent.
    #[command(subcommand)]
    Agent(agent::AgentCommand),
    /// Validate configuration and skills, or run checks once.
    #[command(subcommand)]
    Check(check::CheckCommand),
    /// Test chat and notification delivery.
    #[command(subcommand)]
    Notify(notify::NotifyCommand),
    /// Push a synthetic critical signal through the whole pipeline.
    Selftest(selftest::SelftestArgs),
    /// Build and send the daily digest.
    #[command(subcommand)]
    Digest(digest::DigestCommand),
    /// Print the hub's OpenAPI document as JSON (used by `cargo xtask openapi`).
    #[command(hide = true)]
    Openapi,
    /// Print version and build information.
    Version,
}

impl Command {
    /// Whether this command's output is the point, so logs default to warnings only.
    pub fn is_quiet(&self) -> bool {
        !matches!(
            self,
            Self::Hub(hub::HubCommand::Run(_)) | Self::Agent(agent::AgentCommand::Run(_))
        )
    }
}

/// Runs `command` and returns the process exit code.
pub fn run(command: Command) -> anyhow::Result<ExitCode> {
    match command {
        Command::Version => version::run(),
        Command::Openapi => {
            print!("{}", domovoi_hub::router::openapi_json());
            Ok(ExitCode::SUCCESS)
        }
        Command::Check(command) => check::run(command),
        Command::Hub(command) => block_on(hub::run(command)),
        Command::Agent(command) => block_on(agent::run(command)),
        Command::Notify(command) => block_on(notify::run(command)),
        Command::Selftest(args) => block_on(selftest::run(args)),
        Command::Digest(command) => block_on(digest::run(command)),
    }
}

fn block_on<F: Future<Output = anyhow::Result<ExitCode>>>(future: F) -> anyhow::Result<ExitCode> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(future)
}

/// Prints `err` with its causes on standard error and returns exit code 1.
pub fn fail(err: &anyhow::Error) -> ExitCode {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "error: {err}");
    for cause in err.chain().skip(1) {
        let _ = writeln!(stderr, "  caused by: {cause}");
    }
    ExitCode::FAILURE
}

/// Prints a warning line on standard error.
pub fn warn(message: &str) {
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// The agent configuration at `path`, validated with secrets resolved. Warnings go to
/// standard error; errors fail with every issue listed.
pub fn load_agent(path: &Path) -> anyhow::Result<AgentConfig> {
    loaded(path, domovoi_config::load_agent(path, &SystemEnv))
}

fn loaded<T>(
    path: &Path,
    result: Result<(T, Vec<ConfigIssue>), Vec<ConfigIssue>>,
) -> anyhow::Result<T> {
    match result {
        Ok((config, warnings)) => {
            for issue in warnings {
                warn(&format!("{}: {issue}", path.display()));
            }
            Ok(config)
        }
        Err(issues) => {
            let lines: Vec<String> = issues.iter().map(ToString::to_string).collect();
            anyhow::bail!("{} is not valid:\n  {}", path.display(), lines.join("\n  "))
        }
    }
}

/// A redactor for every secret in `config`.
pub fn redactor<C: Secrets + Clone>(config: &C) -> Arc<dyn Redactor> {
    let mut config = config.clone();
    let mut secrets = Vec::new();
    config.secrets_mut(&mut |_, secret| secrets.push(secret.value().clone()));
    Arc::new(PatternRedactor::new(secrets))
}

/// `--config` for hub commands.
#[derive(Debug, clap::Args)]
pub struct HubConfigArg {
    /// The hub configuration file. Optional: without it, `/etc/domovoi/hub.toml` is read
    /// when it exists, and otherwise the hub runs on defaults, the environment and the
    /// settings saved in the web UI.
    #[arg(long, short, env = "DOMOVOI_HUB_CONFIG")]
    pub config: Option<PathBuf>,
}

impl HubConfigArg {
    /// The file and environment layers. A file named explicitly must exist; the default
    /// one is used only when it does.
    pub fn sources(&self) -> anyhow::Result<ConfigSources> {
        let file = match &self.config {
            Some(path) if path.exists() => Some(path.clone()),
            Some(path) => anyhow::bail!("{} does not exist", path.display()),
            None => Some(PathBuf::from(DEFAULT_HUB_CONFIG)).filter(|p| p.exists()),
        };
        Ok(ConfigSources {
            file,
            env: Arc::new(SystemEnv),
        })
    }

    /// The effective settings: file, environment and the values saved in the web UI.
    /// Warnings go to standard error.
    pub async fn effective(&self) -> anyhow::Result<HubConfig> {
        let sources = self.sources()?;
        let (config, warnings) =
            domovoi_hub::settings::load_effective(&sources, &crate::init::SystemClock).await?;
        for issue in warnings {
            warn(&format!("settings: {issue}"));
        }
        Ok(config)
    }
}
