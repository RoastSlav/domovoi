//! The checks only the agent can make: ST-01, ST-02, ST-12, ST-14, ST-15, ST-16.

use domovoi_core::identity::{CheckToken, GroupEntry, PathFacts, PathKind};
use domovoi_core::isolation::{CheckId, Outcome};
use domovoi_core::ports::Access;
use domovoi_helper::selftest::run_agent_checks;

use super::support::*;

fn fail_token(outcome: &Outcome) -> Option<CheckToken> {
    match outcome {
        Outcome::Fail { token, .. } => Some(*token),
        _ => None,
    }
}

// covers: ST-01
// covers: ST-02
#[test]
fn selftest_agent_checks_pass_for_the_agent_account_with_its_own_groups() {
    let host = agent_host();
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    assert_eq!(result(&results, CheckId::St01).outcome, Outcome::Pass);
    assert_eq!(result(&results, CheckId::St02).outcome, Outcome::Pass);
    assert_eq!(result(&results, CheckId::St15).outcome, Outcome::Pass);
    assert_eq!(result(&results, CheckId::St16).outcome, Outcome::Pass);
}

// covers: ST-01
// covers: ST-02
#[test]
fn selftest_agent_checks_are_nd_when_the_process_is_root_and_st01_fails() {
    let host = healthy_host();
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    assert_eq!(
        fail_token(&result(&results, CheckId::St01).outcome),
        Some(CheckToken::PrivilegeBoundaryNotInEffect)
    );
    for id in [CheckId::St02, CheckId::St14, CheckId::St15, CheckId::St16] {
        assert!(
            result(&results, id).outcome.is_nd(),
            "{id} must be nd for root"
        );
    }
}

// covers: ST-02
#[test]
fn selftest_agent_fails_with_docker_group_among_its_groups() {
    let host = agent_host()
        .groups(&[AGENT_GID, JOURNAL_GID, 999])
        .group(GroupEntry {
            name: "docker".to_owned(),
            gid: 999,
            members: vec![],
        });
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    assert_eq!(
        fail_token(&result(&results, CheckId::St02).outcome),
        Some(CheckToken::AgentGroupsUnexpected)
    );
}

// covers: ST-14
// covers: socket_unreachable
#[test]
fn selftest_fails_when_the_helper_socket_does_not_answer_health() {
    let host = agent_host();
    let health = FakeHealth(Err("connect: no such file".to_owned()));
    let results = run_agent_checks(&host, &health, &dirs());
    assert_eq!(
        fail_token(&result(&results, CheckId::St14).outcome),
        Some(CheckToken::SocketUnreachable)
    );
}

// covers: ST-14
#[test]
fn selftest_reports_the_helper_half_of_st14_and_leaves_the_runner_half_nd() {
    let host = agent_host();
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    let Outcome::Nd { reason } = &result(&results, CheckId::St14).outcome else {
        panic!("ST-14 is nd while the runner socket is");
    };
    assert!(reason.contains("helper socket answered health"), "{reason}");
}

// covers: ST-12
#[test]
fn selftest_fails_on_world_connectable_socket() {
    let socket = dirs().helper_socket;
    let host = agent_host().path(
        socket,
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
    assert_eq!(fail_token(&st12.outcome), Some(CheckToken::SocketModeWrong));
    assert_eq!(st12.faults.len(), 1);
    assert_eq!(st12.faults[0].found.as_ref().map(|f| f.mode), Some(0o666));
    assert_eq!(st12.faults[0].expected.mode, 0o660);
}

// covers: ST-15
// covers: ST-16
// covers: agent_reaches_helper_state
// covers: agent_reaches_docker
#[test]
fn selftest_fails_when_agent_reaches_helper_state_or_docker() {
    let helper = dirs().helper_dir;
    let host = agent_host()
        .access(
            helper.join("attest.key"),
            domovoi_core::ports::Probe::Open,
            Access::Allowed,
        )
        .access(
            format!("/proc/{HELPER_PID}/environ"),
            domovoi_core::ports::Probe::Open,
            Access::Allowed,
        )
        .access(
            "/var/run/docker.sock",
            domovoi_core::ports::Probe::Connect,
            Access::Allowed,
        );
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    let st15 = result(&results, CheckId::St15);
    assert_eq!(
        fail_token(&st15.outcome),
        Some(CheckToken::AgentReachesHelperState)
    );
    let Outcome::Fail { detail, .. } = &st15.outcome else {
        panic!("ST-15 fails");
    };
    assert!(detail.contains("attest.key"), "{detail}");
    assert!(
        detail.contains(&format!("/proc/{HELPER_PID}/environ")),
        "{detail}"
    );
    assert_eq!(
        fail_token(&result(&results, CheckId::St16).outcome),
        Some(CheckToken::AgentReachesDocker)
    );
}

// covers: ST-15
#[test]
fn selftest_st15_is_nd_when_nothing_answers_eacces() {
    let host = agent_host()
        .any_access(dirs().helper_dir.join("attest.key"), Access::NotFound)
        .any_access(dirs().helper_dir.join("state.lock"), Access::NotFound)
        .any_access(dirs().helper_dir, Access::NotFound)
        .any_access("/proc/1/environ", Access::NotFound);
    let results = run_agent_checks(&host, &FakeHealth::ok(), &dirs());
    assert!(result(&results, CheckId::St15).outcome.is_nd());
}
