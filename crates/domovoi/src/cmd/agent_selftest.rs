//! `domovoi agent selftest`: the isolation checks ST-01 to ST-20 on this host.
//!
//! The command is the composition root for the self-test: it injects the host adapter, the
//! executor and the clock, and prints what the helper crate decided. It is not the hub's
//! pipeline self-test (`domovoi selftest`), which is unchanged.

use crate::ExitCode;
use clap::Args;
use std::path::PathBuf;

use super::DEFAULT_AGENT_CONFIG;

#[cfg(unix)]
#[path = "agent_selftest_runner.rs"]
mod runner;

/// `agent selftest`.
#[derive(Debug, Args)]
pub struct SelftestArgs {
    /// Run the root checks here and the agent checks as `domovoi-agent` (needs root).
    #[arg(long)]
    installer: bool,
    /// With `--installer`: write the platform record the helper reads (needs root).
    #[arg(long, requires = "installer")]
    record: bool,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
    /// Print only the agent-run checks as JSON; the installer starts this as the agent account.
    #[arg(long, hide = true)]
    agent_checks: bool,
    /// The agent configuration file, read for the directories when it exists.
    #[arg(long, short, env = "DOMOVOI_AGENT_CONFIG", default_value = DEFAULT_AGENT_CONFIG)]
    config: PathBuf,
}

#[cfg(unix)]
pub async fn run(args: SelftestArgs) -> anyhow::Result<ExitCode> {
    unix::run(args).await
}

#[cfg(not(unix))]
pub fn run(_args: SelftestArgs) -> std::future::Ready<anyhow::Result<ExitCode>> {
    std::future::ready(Err(anyhow::anyhow!(
        "the isolation self-test runs on Linux hosts only"
    )))
}

#[cfg(unix)]
mod unix {
    use super::SelftestArgs;
    use super::runner;
    use crate::ExitCode;
    use crate::init::SystemClock;
    use std::sync::Arc;

    use domovoi_agent::skill_runner::RunnerClient;
    use domovoi_core::identity::HostDirs;
    use domovoi_core::isolation::WriteOptIn;
    use domovoi_core::ports::HostInspect;
    use domovoi_helper::inspect::UnixHostInspect;
    use domovoi_helper::platform::record_platform;
    use domovoi_helper::selftest::{
        AffectedSkill, Mode, RunnerHealth, RunnerObservation, SYSTEMCTL, SelftestEnv,
        affected_skills, agent_checks_json, decide, exit_code, render_json, render_text,
        run_selftest_observed,
    };
    use domovoi_hub::PatternRedactor;
    use domovoi_runner::{SandboxExecutor, SkillCatalog};

    /// Where `helper.toml` is read from.
    const HELPER_TOML: &str = "/etc/domovoi/helper.toml";

    /// The directories of the layout: the table's defaults, with the three the agent's own
    /// configuration may move.
    fn host_dirs(args: &SelftestArgs) -> HostDirs {
        let mut dirs = HostDirs::default();
        if args.config.exists()
            && let Ok(config) = super::super::load_agent(&args.config)
        {
            dirs.state_dir = config.agent.state_dir.clone();
            dirs.skills_dir = config.agent.skills_dir.clone();
            dirs.packages_dir = config.agent.packages_dir.clone();
        }
        dirs
    }

    /// Skills whose manifest does not run them as the skill account, from the catalog loader.
    fn skills_not_on_the_skill_account(dirs: &HostDirs) -> Vec<AffectedSkill> {
        let (catalog, _issues) = SkillCatalog::load(&dirs.skills_dir);
        let pairs: Vec<(String, Option<String>)> = catalog
            .summaries()
            .into_iter()
            .filter_map(|summary| {
                let run_as = catalog.get(&summary.name)?.manifest().run_as.clone();
                Some((summary.name.to_string(), run_as))
            })
            .collect();
        affected_skills(&pairs)
    }

    /// The owner's write opt-in, readable only by root; anything unreadable counts as off.
    fn write_opt_in() -> WriteOptIn {
        let on = domovoi_config::load_helper(std::path::Path::new(HELPER_TOML))
            .map(|(config, _)| config.write_opt_in())
            .unwrap_or(false);
        WriteOptIn(on)
    }

    pub async fn run(args: SelftestArgs) -> anyhow::Result<ExitCode> {
        let dirs = host_dirs(&args);
        let inspect = UnixHostInspect::new();
        let executor = SandboxExecutor::new(Arc::new(PatternRedactor::new(Vec::new())));
        let exe = std::env::current_exe()?;
        let exe = exe.to_string_lossy().into_owned();
        let client = RunnerClient::default_path(Arc::new(SystemClock));
        let is_root = inspect.euid() == 0;
        // Root asks for the isolation probe; the agent account only for `health` (ST-14).
        let (observation, runner_health) = if is_root {
            (runner::observe(&client).await, RunnerHealth::NotInstalled)
        } else {
            (RunnerObservation::NotAsked, runner::health(&client).await)
        };
        let health = runner::RunnerAwareProbe::new(runner_health);
        let env = SelftestEnv {
            inspect: &inspect,
            executor: &executor,
            health: &health,
            dirs: &dirs,
            exe: &exe,
            systemctl: SYSTEMCTL,
        };
        if args.agent_checks {
            println!("{}", agent_checks_json(&env)?);
            return Ok(ExitCode::SUCCESS);
        }
        let mode = if args.installer {
            Mode::Installer
        } else {
            Mode::Local
        };
        let report = run_selftest_observed(&env, mode, &observation).await?;
        let decision = decide(&report, write_opt_in());
        if args.record {
            let record =
                record_platform(&inspect, &SystemClock, &dirs.helper_dir, &report, &decision)?;
            crate::cmd::warn(&format!(
                "recorded the platform ({}) in {}",
                record.tier.as_str(),
                dirs.helper_dir.display()
            ));
        }
        if args.json {
            println!("{}", render_json(&report)?);
        } else {
            let affected = skills_not_on_the_skill_account(&dirs);
            print!("{}", render_text(&report, &decision, &affected));
            if is_root {
                println!(
                    "\nThis ran as root, so the agent-account checks are expected to fail. To check the install, \
                     run it as the agent account: sudo -u domovoi-agent domovoi agent selftest -c /etc/domovoi/agent.toml"
                );
            }
        }
        Ok(ExitCode::from(
            u8::try_from(exit_code(&report)).unwrap_or(1),
        ))
    }
}
