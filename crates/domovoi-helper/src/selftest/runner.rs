//! The runner's halves of ST-05, ST-06 and ST-13, from what the binary observed on its socket.
//!
//! The binary asks the runner (through the agent's client) and reduces the answer to a
//! [`RunnerObservation`]; this module only decides. Nothing here touches a socket, so every
//! mapping is testable without one.

use domovoi_core::identity::{CheckToken, evaluate_runner_groups};
use domovoi_core::isolation::{
    CheckId, CheckResult, IsolationReport, Outcome, RunnerFault, evaluate_isolation,
    evaluate_runner_startup,
};

/// What the runner said when the self-test asked it for a probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerObservation {
    /// The runner answered `probe` with its filesystem observations.
    Report(Box<IsolationReport>),
    /// The runner refused every request because its start check found extra groups.
    GroupsExtra(String),
    /// The runner refused every request because its start check found capabilities or
    /// missing `NoNewPrivs`.
    Privileges(String),
    /// The socket does not exist: no runner is installed.
    NotInstalled,
    /// The runner could not be asked or did not answer well; the text is the reason.
    Unavailable(String),
    /// This process cannot ask (not root): nothing was observed.
    NotAsked,
}

/// ST-05, ST-06 and ST-13 from an isolation report the runner collected, else `nd`.
#[must_use]
pub fn runner_checks(report: Option<&IsolationReport>) -> Vec<CheckResult> {
    let Some(report) = report else {
        return nd_runner("runner_not_installed");
    };
    let status = &report.status;
    let st05 = match evaluate_runner_groups(&status.groups, status.gid) {
        Ok(()) => Outcome::Pass,
        Err(gids) => Outcome::fail(
            CheckToken::RunnerGroupsExtra,
            format!("extra groups {gids:?}"),
        ),
    };
    let privileges: Vec<String> = evaluate_runner_startup(status)
        .into_iter()
        .filter(|f| !matches!(f, RunnerFault::ExtraGroups { .. }))
        .map(|f| f.to_string())
        .collect();
    let st06 = if privileges.is_empty() {
        Outcome::Pass
    } else {
        Outcome::fail(CheckToken::RunnerPrivileges, privileges.join("; "))
    };
    let faults = evaluate_isolation(report);
    let st13 = if faults.is_empty() {
        Outcome::Pass
    } else {
        let text: Vec<String> = faults
            .iter()
            .map(|f| format!("point {}: {}", f.point, f.what))
            .collect();
        Outcome::fail(CheckToken::RunnerNotIsolated, text.join("; "))
    };
    vec![
        CheckResult::new(CheckId::St05, st05),
        CheckResult::new(CheckId::St06, st06),
        CheckResult::new(CheckId::St13, st13),
    ]
}

/// `nd` for ST-05 and ST-06 with `reason`, and ST-13 as an unavailable probe.
fn nd_runner(reason: &str) -> Vec<CheckResult> {
    vec![
        CheckResult::new(CheckId::St05, Outcome::nd(reason)),
        CheckResult::new(CheckId::St06, Outcome::nd(reason)),
        CheckResult::new(CheckId::St13, Outcome::nd("runner_probe_unavailable")),
    ]
}

/// ST-05, ST-06 and ST-13 from what was observed on the runner socket.
///
/// A start-check refusal fails the check it names and leaves the other `nd`: the runner stops at
/// its first fault and answers nothing else, so ST-13 is never a pass.
#[must_use]
pub fn runner_results(observation: &RunnerObservation) -> Vec<CheckResult> {
    match observation {
        RunnerObservation::Report(report) => runner_checks(Some(report)),
        RunnerObservation::GroupsExtra(detail) => refused(
            CheckId::St05,
            CheckToken::RunnerGroupsExtra,
            detail,
            CheckId::St06,
        ),
        RunnerObservation::Privileges(detail) => refused(
            CheckId::St06,
            CheckToken::RunnerPrivileges,
            detail,
            CheckId::St05,
        ),
        RunnerObservation::NotInstalled | RunnerObservation::NotAsked => {
            nd_runner("runner_not_installed")
        }
        RunnerObservation::Unavailable(why) => nd_runner(&format!("runner_unreachable: {why}")),
    }
}

fn refused(failed: CheckId, token: CheckToken, detail: &str, other: CheckId) -> Vec<CheckResult> {
    vec![
        CheckResult::new(failed, Outcome::fail(token, detail)),
        CheckResult::new(
            other,
            Outcome::nd(format!(
                "runner_start_refused: the runner stopped at {token}"
            )),
        ),
        CheckResult::new(CheckId::St13, Outcome::nd("runner_probe_unavailable")),
    ]
}
