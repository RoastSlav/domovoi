//! The isolation self-test, ST-01 to ST-20.
//!
//! Collection lives here; every decision is made by the evaluators in `domovoi-core`. The agent's
//! checks run in the agent's process, the root checks in root, and the runner's checks are `nd`
//! until the runner reports. The installer flow runs the root checks itself and obtains the
//! agent's results by starting the agent checks as the agent account through the injected
//! executor: this crate spawns nothing itself.

mod agent_checks;
mod duration;
mod render;
mod root_checks;
mod runner;
mod units;

use std::collections::BTreeMap;

use domovoi_core::identity::{AGENT_ACCOUNT, HostDirs};
use domovoi_core::isolation::{
    BoundaryReason, BoundaryState, CheckId, CheckResult, IsolationReport, Outcome, RunBy,
    SUPPORTED_PLATFORMS, SelftestReport, TierDecision, WriteOptIn, classify_platform,
    inspect_boundary,
};
use domovoi_core::ports::{HostInspect, ProcessExecutor, ProcessSpec};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};

pub use agent_checks::{
    DOCKER_SOCKET, HealthAnswer, HealthProbe, RunnerHealth, UnixHealthProbe, run_agent_checks,
};
pub use duration::{SystemdDuration, parse_systemd_duration};
pub use render::{
    AffectedSkill, VERDICT_COMMAND, affected_skills, exit_code, render_fault, render_json,
    render_result, render_text, skill_row, unjudged_checks,
};
pub use root_checks::{
    RootChecks, SLOW_WRITE_MICROS, TableFaults, WRITE_CYCLES, detect_systemd, run_root_checks,
    table_faults,
};
pub use runner::{RunnerObservation, runner_checks, runner_results};
pub use units::{
    AGENT_UNIT, HELPER_SOCKET_UNIT, RUNNER_SOCKET_UNIT, SELFTEST_INSTANCE, UnitInspection,
    UnitTextFault, inspect_units, parse_show, scan_unit_text, st07, st09,
};

/// Where `systemctl` lives; the executor runs it by absolute path.
pub const SYSTEMCTL: &str = "/usr/bin/systemctl";

/// Where `setpriv` lives. It switches to the agent account and initialises that account's
/// groups (`--init-groups`), which the executor's `run_as` does not: a child started that way has
/// no supplementary groups, so ST-02 would fail on a correctly set up account.
pub const SETPRIV: &str = "/usr/bin/setpriv";

/// The arguments that make `domovoi` print the agent-run checks as JSON.
pub const AGENT_CHECKS_ARGS: [&str; 4] = ["agent", "selftest", "--agent-checks", "--json"];

/// Everything a self-test reads from or runs on the host, injected by the binary.
pub struct SelftestEnv<'a> {
    /// Read-only host facts.
    pub inspect: &'a dyn HostInspect,
    /// The only way a program is run (`systemctl`, and the agent-checks child).
    pub executor: &'a dyn ProcessExecutor,
    /// How ST-14 asks the helper for `health`.
    pub health: &'a dyn HealthProbe,
    /// Where the host keeps its directories.
    pub dirs: &'a HostDirs,
    /// The absolute path of this binary, for the agent-checks child.
    pub exe: &'a str,
    /// The absolute path of `systemctl`, normally [`SYSTEMCTL`].
    pub systemctl: &'a str,
}

/// How much of the self-test this process runs itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The agent checks in this process, the root checks only when this process is root; the
    /// rest are `nd`.
    Local,
    /// The installer's run: root checks here, the agent checks in a child run as the agent
    /// account. Needs root.
    Installer,
}

impl Mode {
    /// The mode a run takes: the installer's when asked for, and also when root runs a plain
    /// self-test on a host that has the agent account. Root running the agent checks itself
    /// would only describe root (ST-01 fails and the host reads as blocked), never the install.
    #[must_use]
    pub fn for_run(installer: bool, inspect: &dyn HostInspect) -> Self {
        let agent_installed = matches!(inspect.user(AGENT_ACCOUNT), Ok(Some(_)));
        if installer || (inspect.euid() == 0 && agent_installed) {
            Self::Installer
        } else {
            Self::Local
        }
    }
}

/// Why a self-test could not run at all.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelftestError {
    /// `--installer` needs root to run the root checks and to start the child as the agent.
    #[error("the installer self-test must run as root")]
    NotRoot,
}

/// What `domovoi agent selftest --agent-checks --json` prints: the agent's own results and the
/// boundary it observed about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChecksOutput {
    /// Whether this process is the unprivileged account with the expected groups.
    pub boundary: BoundaryState,
    /// ST-01, ST-02, ST-12, ST-14, ST-15 and ST-16.
    pub results: Vec<CheckResult>,
}

/// The agent-run checks of this process, as the `--agent-checks` subcommand prints them.
#[must_use]
pub fn collect_agent_checks(env: &SelftestEnv<'_>) -> AgentChecksOutput {
    AgentChecksOutput {
        boundary: inspect_boundary(env.inspect),
        results: run_agent_checks(env.inspect, env.health, env.dirs),
    }
}

/// The JSON of [`collect_agent_checks`].
pub fn agent_checks_json(env: &SelftestEnv<'_>) -> Result<String, serde_json::Error> {
    serde_json::to_string(&collect_agent_checks(env))
}

fn nd_all(ids: impl Iterator<Item = CheckId>, reason: &str) -> Vec<CheckResult> {
    ids.map(|id| CheckResult::new(id, Outcome::nd(reason)))
        .collect()
}

fn agent_ids() -> impl Iterator<Item = CheckId> {
    CheckId::ALL
        .iter()
        .copied()
        .filter(|i| i.run_by() == RunBy::Agent)
}

/// Starts the agent checks as `domovoi-agent` (through `setpriv`, by the executor) and reads their JSON.
async fn agent_checks_as_agent(env: &SelftestEnv<'_>) -> Result<AgentChecksOutput, String> {
    let mut args: Vec<String> = [
        "--reuid",
        AGENT_ACCOUNT,
        "--regid",
        AGENT_ACCOUNT,
        "--init-groups",
        "--",
        env.exe,
    ]
    .iter()
    .map(|a| (*a).to_owned())
    .collect();
    args.extend(AGENT_CHECKS_ARGS.iter().map(|a| (*a).to_owned()));
    let spec = ProcessSpec {
        program: SETPRIV.to_owned(),
        args,
        env: BTreeMap::new(),
        cwd: None,
        stdin: None,
        timeout: SignedDuration::from_secs(30),
        max_output_bytes: 1 << 20,
        run_as: None,
    };
    let output = env.executor.run(spec).await.map_err(|e| e.to_string())?;
    if output.exit_code != Some(0) || output.timed_out {
        let why = output
            .stderr
            .lines()
            .next()
            .unwrap_or("no message")
            .to_owned();
        return Err(format!(
            "the agent checks exited {:?}: {why}",
            output.exit_code
        ));
    }
    serde_json::from_str(&output.stdout).map_err(|e| format!("unreadable agent checks: {e}"))
}

/// Runs the self-test and returns the merged report, every check present once in id order.
///
/// `runner` is the isolation report the runner collected in probe mode, when there is one.
pub async fn run_selftest(
    env: &SelftestEnv<'_>,
    mode: Mode,
    runner: Option<&IsolationReport>,
) -> Result<SelftestReport, SelftestError> {
    run_selftest_with(env, mode, runner_checks(runner)).await
}

/// Like [`run_selftest`], with the runner's halves taken from what the binary observed on the
/// runner socket.
pub async fn run_selftest_observed(
    env: &SelftestEnv<'_>,
    mode: Mode,
    runner: &RunnerObservation,
) -> Result<SelftestReport, SelftestError> {
    run_selftest_with(env, mode, runner_results(runner)).await
}

async fn run_selftest_with(
    env: &SelftestEnv<'_>,
    mode: Mode,
    runner_results: Vec<CheckResult>,
) -> Result<SelftestReport, SelftestError> {
    let is_root = env.inspect.euid() == 0;
    if mode == Mode::Installer && !is_root {
        return Err(SelftestError::NotRoot);
    }
    let version = detect_systemd(env.inspect, env.executor, env.systemctl).await;
    let mut results = runner_results;
    let boundary = match mode {
        Mode::Local => {
            let out = collect_agent_checks(env);
            results.extend(out.results);
            out.boundary
        }
        Mode::Installer => match agent_checks_as_agent(env).await {
            Ok(out) => {
                results.extend(out.results);
                out.boundary
            }
            Err(why) => {
                results.extend(nd_all(
                    agent_ids(),
                    &format!("agent_checks_unavailable: {why}"),
                ));
                missing_agent_boundary(env)
            }
        },
    };
    if is_root {
        let root = run_root_checks(
            env.inspect,
            env.executor,
            env.dirs,
            env.systemctl,
            version.clone(),
        )
        .await;
        results.extend(root.results);
    } else {
        let ids = CheckId::ALL
            .iter()
            .copied()
            .filter(|i| i.run_by() == RunBy::Root);
        results.extend(nd_all(ids, "needs_root"));
    }
    let results = CheckId::ALL
        .iter()
        .filter_map(|id| results.iter().find(|r| r.id == *id).cloned())
        .collect();
    Ok(SelftestReport {
        results,
        platform: crate::platform::platform_id(env.inspect),
        systemd_version: version,
        boundary,
    })
}

/// The boundary when the child that would have observed it could not run: nothing may claim a
/// boundary nobody looked at.
fn missing_agent_boundary(env: &SelftestEnv<'_>) -> BoundaryState {
    let reason = match env.inspect.user(AGENT_ACCOUNT) {
        Ok(Some(account)) => BoundaryReason::AgentNotDomovoiAgent { uid: account.uid },
        _ => BoundaryReason::AccountMissing,
    };
    BoundaryState::NotInEffect { reason }
}

/// The tier decision for `report`, with `SUPPORTED_PLATFORMS` and the owner's write opt-in.
#[must_use]
pub fn decide(report: &SelftestReport, opt_in: WriteOptIn) -> TierDecision {
    classify_platform(report, SUPPORTED_PLATFORMS, opt_in)
}
