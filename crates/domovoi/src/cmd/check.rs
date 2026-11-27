//! `domovoi check config|skills|run`.

use crate::ExitCode;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Subcommand, ValueEnum};
use domovoi_agent::dry_run::{self, CheckRun};
use domovoi_config::agent::AgentSection;
use domovoi_config::{AgentConfig, ConfigIssue, SystemEnv};
use domovoi_core::CheckKey;
use domovoi_runner::{SandboxExecutor, SkillCatalog};

use super::{DEFAULT_AGENT_CONFIG, load_agent, redactor};
use crate::init::SystemClock;

/// What to check.
#[derive(Debug, Subcommand)]
pub enum CheckCommand {
    /// Load and validate a configuration file; print every issue with its path.
    Config {
        /// The file to check.
        file: PathBuf,
        /// Which kind of file; by default guessed from its `[hub]` or `[agent]` table.
        #[arg(long, value_enum)]
        kind: Option<ConfigKind>,
    },
    /// Load a skills folder; print the valid skills and every issue.
    Skills {
        /// The folder of skills, one sub-folder per skill. Default: `skills_dir` of
        /// `/etc/domovoi/agent.toml` when it can be read, else `/etc/domovoi/skills`.
        dir: Option<PathBuf>,
    },
    /// Run every enabled check once on this host and print what it saw. Needs no hub.
    Run {
        /// The agent configuration file.
        #[arg(long, short, env = "DOMOVOI_AGENT_CONFIG", default_value = DEFAULT_AGENT_CONFIG)]
        config: PathBuf,
        /// Run only this check.
        #[arg(long)]
        only: Option<String>,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// A configuration file's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConfigKind {
    /// `hub.toml`.
    Hub,
    /// `agent.toml`.
    Agent,
    /// `helper.toml`, parsed and validated by the helper's own parser.
    Helper,
}

pub fn run(command: CheckCommand) -> anyhow::Result<ExitCode> {
    match command {
        CheckCommand::Config { file, kind } => config(&file, kind),
        CheckCommand::Skills { dir } => skills(&dir.unwrap_or_else(default_skills_dir)),
        CheckCommand::Run { config, only, json } => super::block_on(run_checks(config, only, json)),
    }
}

fn guess_kind(file: &Path) -> anyhow::Result<ConfigKind> {
    let text = std::fs::read_to_string(file)
        .map_err(|err| anyhow::anyhow!("cannot read {}: {err}", file.display()))?;
    let has = |table: &str| {
        text.lines()
            .any(|line| line.trim_start().starts_with(&format!("[{table}]")))
    };
    // A helper file has no table that names it, and may hold `groups` only.
    let named_helper = file.file_name().is_some_and(|name| name == "helper.toml");
    let helper_keys = text
        .lines()
        .any(|line| line.trim_start().starts_with("groups"))
        || ["platform", "safety", "smart", "limits", "nft"]
            .iter()
            .any(|table| has(table));
    match (has("hub"), has("agent")) {
        (true, false) => Ok(ConfigKind::Hub),
        (false, true) => Ok(ConfigKind::Agent),
        (false, false) if named_helper || helper_keys => Ok(ConfigKind::Helper),
        _ => anyhow::bail!(
            "cannot tell whether {} is a hub, an agent or a helper configuration; pass --kind",
            file.display()
        ),
    }
}

fn config(file: &Path, kind: Option<ConfigKind>) -> anyhow::Result<ExitCode> {
    let kind = match kind {
        Some(kind) => kind,
        None => guess_kind(file)?,
    };
    let issues = match kind {
        ConfigKind::Hub => issues(domovoi_config::load_hub(file, &SystemEnv)),
        ConfigKind::Agent => issues(domovoi_config::load_agent(file, &SystemEnv)),
        // The same parser the helper's start check runs, so it says what the helper would.
        ConfigKind::Helper => issues(domovoi_config::load_helper(file)),
    };
    for issue in &issues {
        println!("{issue}");
    }
    hint_permissions(issues.iter().map(ToString::to_string));
    let errors = issues.iter().filter(|i| i.is_error()).count();
    if errors > 0 {
        println!("{}: {errors} error(s)", file.display());
        return Ok(ExitCode::FAILURE);
    }
    println!("{}: ok ({} warning(s))", file.display(), issues.len());
    Ok(ExitCode::SUCCESS)
}

fn issues<T>(result: Result<(T, Vec<ConfigIssue>), Vec<ConfigIssue>>) -> Vec<ConfigIssue> {
    match result {
        Ok((_, warnings)) => warnings,
        Err(issues) => issues,
    }
}

/// The skills folder of this host: where the agent's own configuration says, else the default.
fn default_skills_dir() -> PathBuf {
    let path = std::env::var_os("DOMOVOI_AGENT_CONFIG")
        .map_or_else(|| PathBuf::from(DEFAULT_AGENT_CONFIG), PathBuf::from);
    domovoi_config::load_agent(&path, &SystemEnv).map_or_else(
        |_| AgentSection::default_dirs().skills_dir,
        |(config, _)| config.agent.skills_dir,
    )
}

fn skills(dir: &Path) -> anyhow::Result<ExitCode> {
    let (catalog, issues) = SkillCatalog::load(dir);
    let summaries = catalog.summaries();
    for skill in &summaries {
        println!(
            "ok     {} ({}): {}",
            skill.name,
            format!("{:?}", skill.risk).to_lowercase(),
            skill.description
        );
    }
    for issue in &issues {
        println!("error  {issue}");
    }
    println!(
        "{}: {} valid, {} with issues",
        dir.display(),
        summaries.len(),
        issues.len()
    );
    hint_permissions(issues.iter().map(ToString::to_string));
    Ok(if issues.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Says once what to do when any of `messages` reports a refused file.
fn hint_permissions(messages: impl Iterator<Item = String>) {
    let mut messages = messages;
    if let Some(hint) = messages.find_map(|message| super::permission_hint(&message)) {
        super::warn(hint);
    }
}

async fn run_checks(path: PathBuf, only: Option<String>, json: bool) -> anyhow::Result<ExitCode> {
    let config: AgentConfig = load_agent(&path)?;
    let only = only
        .map(|key| CheckKey::parse(&key))
        .transpose()
        .map_err(|err| anyhow::anyhow!("--only: {err}"))?;
    let redactor = redactor(&config);
    let deps = domovoi_agent::production::deps(
        &config,
        Arc::new(SandboxExecutor::new(redactor.clone())),
        Arc::new(SystemClock),
        redactor,
        None,
    )?;
    let runs = dry_run::run_checks_once(&config, &deps, only.as_ref()).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
    } else {
        print_runs(&runs);
    }
    // A check that cannot run here (a missing tool, say) is reported, not fatal: the run
    // itself completed.
    Ok(ExitCode::SUCCESS)
}

fn print_runs(runs: &[CheckRun]) {
    for run in runs {
        if let Some(err) = &run.error {
            println!("{}: error: {err}", run.check);
            continue;
        }
        if let Some(why) = &run.unavailable {
            println!("{}: unavailable: {why}", run.check);
            continue;
        }
        println!("{}:", run.check);
        for o in &run.observations {
            let state = if o.firing { "FIRING" } else { "ok" };
            let target = o
                .target
                .as_deref()
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();
            println!("  {state:<6} {}{target}: {}", o.severity, o.summary);
        }
        for f in &run.facts {
            let unit = f
                .unit
                .as_deref()
                .map(|u| format!(" {u}"))
                .unwrap_or_default();
            println!("  fact   {} = {}{unit}", f.label, f.value);
        }
    }
    let summary: Vec<String> = dry_run::summary(runs)
        .iter()
        .map(|(label, n)| format!("{n} {label}"))
        .collect();
    println!("{} checks: {}", runs.len(), summary.join(", "));
}
