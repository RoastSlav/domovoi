//! The runner's halves of the self-test: ST-05, ST-06, ST-07, ST-12, ST-13, ST-14 and the SK-01 rows.

use domovoi_core::identity::{CheckToken, PathFacts, PathKind, SKILL_ACCOUNT};
use domovoi_core::isolation::{
    BoundaryReason, BoundaryState, CheckId, Outcome, PlatformId, SelftestReport, WriteOptIn,
};
use domovoi_core::skill_runner::RUNNER_SOCKET_PATH;
use domovoi_helper::selftest::{
    AffectedSkill, HealthAnswer, HealthProbe, Mode, RunnerHealth, RunnerObservation,
    SELFTEST_INSTANCE, SYSTEMCTL, affected_skills, decide, inspect_units, render_text,
    run_agent_checks, run_selftest_observed, st07,
};
use domovoi_testkit::FakeProcessExecutor;

use super::support::*;

const RUNNER_SOCKET: &str = "domovoi-skill-runner.socket";
const RUNNER_SERVICE: &str = "domovoi-skill-runner@selftest.service";
const RUNNER_SOCKET_PROPS: &str = "Accept,SocketMode,SocketUser,SocketGroup";
const RUNNER_SERVICE_PROPS: &str = "Type,User,RuntimeMaxUSec,ExecStart,ProcSubset,ProtectProc,NoNewPrivileges,CapabilityBoundingSet,BindReadOnlyPaths";

fn runner_socket_show() -> String {
    "LoadState=loaded\nFragmentPath=/etc/systemd/system/domovoi-skill-runner.socket\nDropInPaths=\nNeedDaemonReload=no\nAccept=yes\nSocketMode=0660\nSocketUser=root\nSocketGroup=domovoi-agent\n".to_owned()
}

fn runner_service_show(skills: &str, packages: &str) -> String {
    format!(
        "LoadState=loaded\nFragmentPath=/etc/systemd/system/domovoi-skill-runner@.service\nDropInPaths=\nNeedDaemonReload=no\nType=simple\nUser={SKILL_ACCOUNT}\nRuntimeMaxUSec=1h 2min\nExecStart={{ path=/usr/local/bin/domovoi ; argv[]=/usr/local/bin/domovoi skill-runner serve --skills-dir {skills} --packages-dir {packages} ; ignore_errors=no ; status=0/0 }}\nProcSubset=all\nProtectProc=invisible\nNoNewPrivileges=yes\nCapabilityBoundingSet=\nBindReadOnlyPaths={skills}:{skills}:rbind {packages}:{packages}:rbind\n"
    )
}

fn executor_with_runner(skills: &str, packages: &str) -> FakeProcessExecutor {
    let exec = healthy_executor();
    expect_show(
        &exec,
        RUNNER_SOCKET,
        RUNNER_SOCKET_PROPS,
        &runner_socket_show(),
    );
    expect_show(
        &exec,
        RUNNER_SERVICE,
        RUNNER_SERVICE_PROPS,
        &runner_service_show(skills, packages),
    );
    exec
}

fn failed_token(outcome: &Outcome) -> Option<CheckToken> {
    match outcome {
        Outcome::Fail { token, .. } => Some(*token),
        _ => None,
    }
}

async fn observed(observation: &RunnerObservation) -> SelftestReport {
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let d = dirs();
    let health = FakeHealth::ok();
    run_selftest_observed(
        &env(&host, &exec, &health, &d),
        Mode::Installer,
        observation,
    )
    .await
    .expect("report")
}

// covers: runner_groups_extra
// covers: runner_privileges
#[tokio::test]
async fn selftest_turns_runner_start_refusals_into_st05_and_st06_failures() {
    let report = observed(&RunnerObservation::GroupsExtra("extra groups [7]".into())).await;
    let st05 = report.result(CheckId::St05).expect("ST-05");
    assert_eq!(
        failed_token(&st05.outcome),
        Some(CheckToken::RunnerGroupsExtra)
    );
    assert!(report.result(CheckId::St13).expect("ST-13").outcome.is_nd());

    let report = observed(&RunnerObservation::Privileges("NoNewPrivs is off".into())).await;
    let st06 = report.result(CheckId::St06).expect("ST-06");
    assert_eq!(
        failed_token(&st06.outcome),
        Some(CheckToken::RunnerPrivileges)
    );
    assert!(report.result(CheckId::St13).expect("ST-13").outcome.is_nd());
    assert!(report.any_failed());
}

#[tokio::test]
async fn selftest_st13_is_nd_when_the_probe_connection_fails() {
    let report = observed(&RunnerObservation::Unavailable("timed out".into())).await;
    let Outcome::Nd { reason } = &report.result(CheckId::St13).expect("ST-13").outcome else {
        panic!("ST-13 must be nd when the probe cannot connect");
    };
    assert_eq!(reason, "runner_probe_unavailable");
    for id in [CheckId::St05, CheckId::St06] {
        assert!(report.result(id).expect("r").outcome.is_nd(), "{id}");
    }
    assert!(!report.any_failed());
}

// covers: unit_modified
#[tokio::test]
async fn selftest_st07_flags_runner_bind_paths_that_differ_from_agent_toml() {
    let d = dirs();
    let skills = d.skills_dir.to_string_lossy().into_owned();
    let packages = d.packages_dir.to_string_lossy().into_owned();

    let exec = executor_with_runner(&skills, &packages);
    let found = inspect_units(&exec, SYSTEMCTL, &d).await;
    assert!(found.iter().any(|u| u.unit == RUNNER_SERVICE));
    assert_eq!(st07(&found).outcome, Outcome::Pass, "{found:?}");

    let exec = executor_with_runner("/srv/other-skills", &packages);
    let found = inspect_units(&exec, SYSTEMCTL, &d).await;
    let Outcome::Fail { token, detail } = st07(&found).outcome else {
        panic!("a runner unit rendered for other directories must fail ST-07");
    };
    assert_eq!(token, CheckToken::UnitModified);
    assert!(detail.contains("bind_paths"), "{detail}");
}

// covers: unit_modified
#[tokio::test]
async fn selftest_st07_flags_a_runner_unit_from_before_the_proc_fix() {
    let d = dirs();
    let skills = d.skills_dir.to_string_lossy().into_owned();
    let packages = d.packages_dir.to_string_lossy().into_owned();
    let exec = healthy_executor();
    expect_show(
        &exec,
        RUNNER_SOCKET,
        RUNNER_SOCKET_PROPS,
        &runner_socket_show(),
    );
    let stale = runner_service_show(&skills, &packages).replace("ProcSubset=all", "ProcSubset=pid");
    expect_show(&exec, RUNNER_SERVICE, RUNNER_SERVICE_PROPS, &stale);
    let found = inspect_units(&exec, SYSTEMCTL, &d).await;
    let Outcome::Fail { token, detail } = st07(&found).outcome else {
        panic!("a runner unit with ProcSubset=pid must fail ST-07");
    };
    assert_eq!(token, CheckToken::UnitModified);
    assert!(detail.contains("ProcSubset=\"pid\""), "{detail}");
}

#[tokio::test]
async fn selftest_st07_ignores_runner_units_that_are_not_installed() {
    let d = dirs();
    let exec = healthy_executor();
    let found = inspect_units(&exec, SYSTEMCTL, &d).await;
    assert!(found.iter().all(|u| !u.unit.contains("skill-runner")));
    assert_eq!(st07(&found).outcome, Outcome::Pass);
    assert_eq!(SELFTEST_INSTANCE, "selftest");
}

fn bare_report(boundary: BoundaryState) -> SelftestReport {
    SelftestReport {
        results: Vec::new(),
        platform: PlatformId {
            os_id: "ubuntu".into(),
            version_id: "24.04".into(),
            arch: "x86_64".into(),
        },
        systemd_version: None,
        boundary,
    }
}

fn listing(boundary: BoundaryState) -> String {
    let skills = vec![
        ("ok".to_owned(), Some(SKILL_ACCOUNT.to_owned())),
        ("legacy".to_owned(), Some("root".to_owned())),
        ("plain".to_owned(), None),
    ];
    let affected: Vec<AffectedSkill> = affected_skills(&skills);
    let report = bare_report(boundary);
    render_text(&report, &decide(&report, WriteOptIn(false)), &affected)
}

#[test]
fn selftest_lists_run_as_unsupported_skills_as_warn() {
    let text = listing(BoundaryState::InEffect);
    let row = text
        .lines()
        .find(|l| l.contains("skill legacy"))
        .expect("row for legacy");
    assert!(row.starts_with("SK-01 warn run_as_unsupported"), "{row}");
    assert!(row.contains("names run_as root"), "{row}");
    let plain = text
        .lines()
        .find(|l| l.contains("skill plain"))
        .expect("row");
    assert!(plain.starts_with("SK-01 info"), "{plain}");
    assert!(!text.contains("skill ok"));
}

#[test]
fn selftest_lists_run_as_as_info_on_a_root_agent() {
    let text = listing(BoundaryState::NotInEffect {
        reason: BoundaryReason::AgentIsRoot,
    });
    let row = text
        .lines()
        .find(|l| l.contains("skill legacy"))
        .expect("row for legacy");
    assert!(row.starts_with("SK-01 info"), "{row}");
    assert!(row.contains("honoured by the legacy path"), "{row}");
    assert!(!text.contains("run_as_unsupported"), "{text}");
}

struct RunnerHealthProbe(RunnerHealth);

impl HealthProbe for RunnerHealthProbe {
    fn health(&self, _socket: &std::path::Path) -> Result<HealthAnswer, String> {
        Ok(HealthAnswer { pid: HELPER_PID })
    }
    fn runner_health(&self) -> RunnerHealth {
        self.0.clone()
    }
}

// covers: ST-14
#[test]
fn selftest_st14_reports_the_runner_health_connect() {
    let host = agent_host();
    let answered = run_agent_checks(&host, &RunnerHealthProbe(RunnerHealth::Answered), &dirs());
    assert_eq!(result(&answered, CheckId::St14).outcome, Outcome::Pass);
    let broken = run_agent_checks(
        &host,
        &RunnerHealthProbe(RunnerHealth::Unreachable("timed out".into())),
        &dirs(),
    );
    assert_eq!(
        failed_token(&result(&broken, CheckId::St14).outcome),
        Some(CheckToken::SocketUnreachable)
    );
}

// covers: ST-12
#[test]
fn selftest_st12_fails_on_a_world_connectable_runner_socket() {
    let host = agent_host().path(
        RUNNER_SOCKET_PATH,
        PathFacts {
            kind: Some(PathKind::Socket),
            symlink: false,
            mode: 0o666,
            uid: 0,
            gid: AGENT_GID,
        },
    );
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    let st12 = result(&results, CheckId::St12);
    assert_eq!(
        failed_token(&st12.outcome),
        Some(CheckToken::SocketModeWrong)
    );
    assert_eq!(st12.faults.len(), 1);
}
