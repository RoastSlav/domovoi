//! The merged self-test and the root-run checks.

use domovoi_core::identity::{
    AGENT_ACCOUNT, CheckToken, GroupEntry, PathFacts, PathKind, SKILL_ACCOUNT,
};
use domovoi_core::isolation::{BoundaryState, CheckId, Outcome, RunBy, SelftestReport, WriteOptIn};
use domovoi_core::ports::ProcessError;
use domovoi_helper::selftest::{
    AffectedSkill, Mode, SelftestError, affected_skills, decide, exit_code, render_json,
    render_text, run_selftest,
};
use domovoi_testkit::{FakeHostInspect, FakeProcessExecutor};

use super::support::*;

fn token(outcome: &Outcome) -> Option<CheckToken> {
    match outcome {
        Outcome::Fail { token, .. } | Outcome::Warn { token, .. } => Some(*token),
        _ => None,
    }
}

async fn installer_report(host: &FakeHostInspect, exec: &FakeProcessExecutor) -> SelftestReport {
    let d = dirs();
    let health = FakeHealth::ok();
    run_selftest(&env(host, exec, &health, &d), Mode::Installer, None)
        .await
        .expect("report")
}

#[tokio::test]
async fn selftest_reports_every_check_with_its_id() {
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let report = installer_report(&host, &exec).await;
    let ids: Vec<CheckId> = report.results.iter().map(|r| r.id).collect();
    assert_eq!(ids, CheckId::ALL);
    for r in &report.results {
        match &r.outcome {
            Outcome::Pass => {}
            Outcome::Nd { reason } => assert!(!reason.is_empty(), "{} has no reason", r.id),
            Outcome::Info { detail }
            | Outcome::Fail { detail, .. }
            | Outcome::Warn { detail, .. } => assert!(!detail.is_empty(), "{} has no detail", r.id),
        }
    }
    assert_eq!(report.boundary, BoundaryState::InEffect);
    assert_eq!(
        report.systemd_version.as_deref(),
        Some("systemd 255 (255.4-1ubuntu8)")
    );
    assert_eq!(report.platform.os_id, "ubuntu");
    assert_eq!(report.platform.version_id, "24.04");
}

// covers: ST-01
// covers: ST-02
#[tokio::test]
async fn selftest_installer_obtains_agent_results_by_running_the_agent_checks_as_the_agent_account()
{
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let report = installer_report(&host, &exec).await;
    let child: Vec<_> = exec
        .calls()
        .into_iter()
        .filter(|c| c.program == domovoi_helper::selftest::SETPRIV)
        .collect();
    assert_eq!(child.len(), 1);
    assert_eq!(
        child[0].run_as, None,
        "setpriv switches to the account itself"
    );
    assert_eq!(child[0].args, CHILD_ARGS);
    assert!(child[0].args.iter().any(|a| a == "--init-groups"));
    for id in [
        CheckId::St01,
        CheckId::St02,
        CheckId::St12,
        CheckId::St15,
        CheckId::St16,
    ] {
        let r = report.result(id).expect("result");
        assert_eq!(r.outcome, Outcome::Pass, "{id}");
        assert_eq!(r.run_by, RunBy::Agent);
    }
}

#[tokio::test]
async fn selftest_installer_marks_agent_checks_nd_when_the_child_cannot_run() {
    let host = healthy_host();
    let exec = healthy_executor();
    exec.expect_result(
        domovoi_helper::selftest::SETPRIV,
        &CHILD_ARGS,
        Err(ProcessError::UserSwitch("no such user".to_owned())),
    );
    let report = installer_report(&host, &exec).await;
    assert!(report.result(CheckId::St02).expect("r").outcome.is_nd());
    assert!(report.boundary != BoundaryState::InEffect);
}

#[tokio::test]
async fn selftest_installer_refuses_to_run_without_root() {
    let host = agent_host();
    let exec = healthy_executor();
    let d = dirs();
    let health = FakeHealth::ok();
    let out = run_selftest(&env(&host, &exec, &health, &d), Mode::Installer, None).await;
    assert_eq!(out.err(), Some(SelftestError::NotRoot));
    assert!(exec.calls().is_empty());
}

#[tokio::test]
async fn selftest_local_run_as_the_agent_reports_the_root_checks_nd() {
    let host = agent_host();
    let exec = healthy_executor();
    let d = dirs();
    let health = FakeHealth::ok();
    let report = run_selftest(&env(&host, &exec, &health, &d), Mode::Local, None)
        .await
        .expect("report");
    for id in [CheckId::St03, CheckId::St10, CheckId::St11, CheckId::St20] {
        let Outcome::Nd { reason } = &report.result(id).expect("r").outcome else {
            panic!("{id} should be nd without root");
        };
        assert_eq!(reason, "needs_root");
    }
    assert_eq!(
        report.result(CheckId::St01).expect("r").outcome,
        Outcome::Pass
    );
}

// covers: ST-05
// covers: ST-06
// covers: ST-13
#[tokio::test]
async fn selftest_marks_runner_checks_nd_until_the_runner_reports() {
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let report = installer_report(&host, &exec).await;
    for id in [CheckId::St05, CheckId::St06, CheckId::St13] {
        assert!(report.result(id).expect("r").outcome.is_nd(), "{id}");
        assert_eq!(report.result(id).expect("r").run_by, RunBy::Runner);
    }
    let decision = decide(&report, WriteOptIn(false));
    assert!(decision.blocking.contains(&CheckId::St05));
    assert_eq!(
        decision.tier,
        domovoi_core::isolation::PlatformTier::Blocked
    );
}

// covers: ST-03
// covers: ST-04
#[tokio::test]
async fn selftest_fails_on_a_skill_account_with_an_extra_group_and_on_docker_membership() {
    let host = healthy_host()
        .user(skill_user(), &[SKILL_GID, 999])
        .group(GroupEntry {
            name: "docker".to_owned(),
            gid: 999,
            members: vec![AGENT_ACCOUNT.to_owned()],
        });
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let report = installer_report(&host, &exec).await;
    assert_eq!(
        token(&report.result(CheckId::St03).expect("r").outcome),
        Some(CheckToken::SkillAccountInvalid)
    );
    assert_eq!(
        token(&report.result(CheckId::St04).expect("r").outcome),
        Some(CheckToken::DockerGroupMember)
    );
}

// covers: agent_account_missing
#[tokio::test]
async fn selftest_fails_with_agent_account_missing_when_the_account_does_not_exist() {
    let host = FakeHostInspect::new()
        .ids(0, 0)
        .user(skill_user(), &[SKILL_GID])
        .path("/run/systemd/system", dir_facts(0o755, 0, 0));
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let report = installer_report(&host, &exec).await;
    for id in [CheckId::St03, CheckId::St10, CheckId::St11] {
        assert_eq!(
            token(&report.result(id).expect("r").outcome),
            Some(CheckToken::AgentAccountMissing),
            "{id}"
        );
    }
}

// covers: ST-10
// covers: ST-11
// covers: ST-12
#[tokio::test]
async fn selftest_reports_group_path_and_socket_faults_with_found_and_expected() {
    let d = dirs();
    let host = healthy_host()
        .path(d.etc_dir.clone(), dir_facts(0o750, 0, 0))
        .path(d.helper_dir.clone(), dir_facts(0o755, 0, 0))
        .path(
            d.helper_socket.clone(),
            PathFacts {
                kind: Some(PathKind::Socket),
                symlink: false,
                mode: 0o666,
                uid: 0,
                gid: AGENT_GID,
            },
        );
    let exec = healthy_executor();
    let report = installer_report(&host, &exec).await;
    let st10 = report.result(CheckId::St10).expect("r");
    assert_eq!(token(&st10.outcome), Some(CheckToken::PathModeWrong));
    assert_eq!(st10.faults.len(), 1);
    assert_eq!(st10.faults[0].path, d.etc_dir);
    assert_eq!(
        st10.faults[0].found.as_ref().map(|f| (f.mode, f.gid)),
        Some((0o750, 0))
    );
    assert_eq!(st10.faults[0].expected.gid, AGENT_GID);
    let st11 = report.result(CheckId::St11).expect("r");
    assert_eq!(
        token(&st11.outcome),
        Some(CheckToken::HelperStatePermissions)
    );
    assert_eq!(st11.faults[0].found.as_ref().map(|f| f.mode), Some(0o755));
    assert_eq!(st11.faults[0].expected.mode, 0o700);
    let text = render_text(&report, &decide(&report, WriteOptIn(false)), &[]);
    assert!(text.contains("ST-10 fail path_mode_wrong"), "{text}");
    assert!(
        text.contains("found directory mode 0750 owner 0:0"),
        "{text}"
    );
    assert!(
        text.contains("expected directory mode 0750 owner 0:990"),
        "{text}"
    );
    assert!(
        text.contains("ST-11 fail helper_state_permissions"),
        "{text}"
    );
}

// covers: ST-08
#[tokio::test]
async fn selftest_records_systemd_version_and_compares_nothing() {
    for version in ["systemd 249 (249.11-0ubuntu3)", "systemd 257 (257.9-1)"] {
        let host = healthy_host();
        let exec = healthy_executor();
        exec.expect(
            domovoi_helper::selftest::SYSTEMCTL,
            &["--version"],
            ok_output(&format!("{version}\n+PAM\n")),
        );
        expect_child(&exec, &child_json());
        let report = installer_report(&host, &exec).await;
        assert_eq!(
            report.result(CheckId::St08).expect("r").outcome,
            Outcome::info(version)
        );
    }
}

// covers: ST-18
// covers: host_directory_created_by_mask
#[tokio::test]
async fn selftest_fails_when_etc_domovoi_is_an_empty_directory_made_by_a_mask() {
    let d = dirs();
    let host = healthy_host()
        .path(d.etc_dir.clone(), dir_facts(0o755, 0, 0))
        .dir(d.etc_dir.clone(), &[]);
    let exec = healthy_executor();
    let report = installer_report(&host, &exec).await;
    assert_eq!(
        token(&report.result(CheckId::St18).expect("r").outcome),
        Some(CheckToken::HostDirectoryCreatedByMask)
    );
    let healthy = installer_report(&healthy_host(), &healthy_executor()).await;
    assert_eq!(
        healthy.result(CheckId::St18).expect("r").outcome,
        Outcome::Pass
    );
}

// covers: ST-17
// covers: container_escape_surface
#[tokio::test]
async fn selftest_fails_on_a_container_install_with_a_writable_docker_socket() {
    let mountinfo = "36 35 0:30 / /var/run/docker.sock rw,relatime - tmpfs tmpfs rw\n";
    let host = healthy_host()
        .path("/.dockerenv", dir_facts(0o755, 0, 0))
        .text("/proc/self/mountinfo", mountinfo)
        .text(
            "/proc/self/status",
            "Uid:\t0\t0\t0\t0\nGid:\t0\t0\t0\t0\nGroups:\t\nCapEff:\t0000000000000000\nCapBnd:\t0000000000000000\nNoNewPrivs:\t0\n",
        );
    let exec = healthy_executor();
    let report = installer_report(&host, &exec).await;
    assert_eq!(
        token(&report.result(CheckId::St17).expect("r").outcome),
        Some(CheckToken::ContainerEscapeSurface)
    );
    let host_install = installer_report(&healthy_host(), &healthy_executor()).await;
    let Outcome::Nd { reason } = &host_install.result(CheckId::St17).expect("r").outcome else {
        panic!("a host install is nd");
    };
    assert_eq!(reason, "not_a_container_install");
}

// covers: ST-19
#[tokio::test]
async fn selftest_records_kernel_security_facts_without_comparing_them() {
    let host = healthy_host()
        .text(
            "/sys/kernel/security/lsm",
            "lockdown,capability,yama,apparmor\n",
        )
        .text("/proc/sys/kernel/yama/ptrace_scope", "1\n")
        .text(
            "/proc/self/mountinfo",
            "30 25 0:25 / /run rw,nosuid,nodev,noexec - tmpfs tmpfs rw\n",
        );
    let report = installer_report(&host, &healthy_executor()).await;
    let Outcome::Info { detail } = &report.result(CheckId::St19).expect("r").outcome else {
        panic!("ST-19 only records");
    };
    assert!(
        detail.contains("lsm=lockdown,capability,yama,apparmor"),
        "{detail}"
    );
    assert!(detail.contains("yama_ptrace_scope=1"), "{detail}");
    assert!(
        detail.contains("/run options=rw,nosuid,nodev,noexec"),
        "{detail}"
    );
}

// covers: ST-20
// covers: state_write_slow
#[tokio::test]
async fn selftest_warns_on_slow_state_writes_and_never_blocks() {
    let host = healthy_host().timings(&[250_000; 10]);
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let mut report = installer_report(&host, &exec).await;
    let st20 = report.result(CheckId::St20).expect("r");
    assert_eq!(token(&st20.outcome), Some(CheckToken::StateWriteSlow));
    assert!(matches!(st20.outcome, Outcome::Warn { .. }));
    assert_eq!(exit_code(&report), 0, "a warning is not a failure");
    // A warning is never a blocking required check: make every required check pass and the
    // tier stays Untested despite ST-20 warning.
    for r in &mut report.results {
        if domovoi_core::isolation::REQUIRED_CHECKS.contains(&r.id) {
            r.outcome = Outcome::Pass;
        }
    }
    let decision = decide(&report, WriteOptIn(false));
    assert_eq!(
        decision.tier,
        domovoi_core::isolation::PlatformTier::Untested
    );
    let fast = installer_report(&healthy_host(), &healthy_executor()).await;
    assert_eq!(
        fast.result(CheckId::St20).expect("r").outcome,
        Outcome::Pass
    );
}

#[tokio::test]
async fn selftest_text_and_json_carry_the_boundary_tier_and_exit_code() {
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let report = installer_report(&host, &exec).await;
    let decision = decide(&report, WriteOptIn(false));
    let text = render_text(&report, &decision, &[]);
    assert!(text.contains("ST-01 pass"), "{text}");
    assert!(text.contains("ST-05 nd runner_not_installed"), "{text}");
    assert!(text.contains("boundary: in effect"), "{text}");
    assert!(text.contains("tier: blocked"), "{text}");
    assert!(text.contains("blocking: ST-05"), "{text}");
    let json: SelftestReport =
        serde_json::from_str(&render_json(&report).expect("json")).expect("parse");
    assert_eq!(json, report);
    assert_eq!(exit_code(&report), 0);
    let mut failed = report.clone();
    failed.results[0].outcome = Outcome::fail(CheckToken::PathModeWrong, "x");
    assert_eq!(exit_code(&failed), 1);
}

#[test]
fn selftest_lists_skills_whose_run_as_is_not_domovoi_skill() {
    let skills = vec![
        ("disk-check".to_owned(), Some(SKILL_ACCOUNT.to_owned())),
        ("legacy-root".to_owned(), Some("root".to_owned())),
        ("plain".to_owned(), None),
    ];
    let affected = affected_skills(&skills);
    assert_eq!(
        affected,
        vec![
            AffectedSkill {
                name: "legacy-root".to_owned(),
                run_as: Some("root".to_owned())
            },
            AffectedSkill {
                name: "plain".to_owned(),
                run_as: None
            },
        ]
    );
    let host_report = SelftestReport {
        results: Vec::new(),
        platform: domovoi_core::isolation::PlatformId {
            os_id: "ubuntu".into(),
            version_id: "24.04".into(),
            arch: "x86_64".into(),
        },
        systemd_version: None,
        boundary: BoundaryState::InEffect,
    };
    let text = render_text(
        &host_report,
        &decide(&host_report, WriteOptIn(false)),
        &affected,
    );
    assert!(text.contains("skill legacy-root runs as root"), "{text}");
    assert!(
        text.contains("skill plain runs as the agent (no run_as)"),
        "{text}"
    );
    assert!(!text.contains("disk-check"));
}

// covers: ST-09
#[tokio::test]
async fn selftest_spawns_only_through_the_executor() {
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    installer_report(&host, &exec).await;
    for call in exec.calls() {
        assert!(
            call.program == domovoi_helper::selftest::SETPRIV
                || call.program == domovoi_helper::selftest::SYSTEMCTL,
            "unexpected program {}",
            call.program
        );
        assert!(
            call.env.is_empty(),
            "the child gets no inherited environment"
        );
    }
}
