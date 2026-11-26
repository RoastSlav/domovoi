//! Tests for the self-test model, the isolation evaluator, the boundary and the tiers.

use std::path::PathBuf;

use super::*;
use crate::identity::{AGENT_ACCOUNT, CheckToken, GroupEntry, HostDirs, JOURNAL_GROUP, UserEntry};
use crate::ports::{Access, HostInspect, InspectError, Probe, TreeEntry};

fn p(text: &str) -> PathBuf {
    PathBuf::from(text)
}

fn rec(path: &str, how: Probe, access: Access) -> AccessRecord {
    AccessRecord::new(path, how, access)
}

fn good_report() -> IsolationReport {
    let dirs = HostDirs::default();
    let mut writes = Vec::new();
    for dir in ["/etc/domovoi/skills", "/var/lib/domovoi-packages"] {
        for how in [Probe::Create, Probe::Rename, Probe::Unlink] {
            writes.push(rec(dir, how, Access::ReadOnly));
        }
    }
    writes.push(rec(
        "/etc/domovoi/skills/a/run.sh",
        Probe::Append,
        Access::ReadOnly,
    ));
    IsolationReport {
        dirs,
        connects: vec![
            rec("/run/domovoi-helper.sock", Probe::Connect, Access::NotFound),
            rec(
                "/run/domovoi-skill-runner.sock",
                Probe::Connect,
                Access::NotFound,
            ),
        ],
        hidden_state: vec![
            rec("/var/lib/domovoi-agent", Probe::Stat, Access::NotFound),
            rec("/var/lib/domovoi-agent", Probe::List, Access::NotFound),
            rec("/var/lib/domovoi-helper", Probe::Open, Access::NotFound),
            rec(
                "/var/lib/domovoi-helper/attest.key",
                Probe::Open,
                Access::NotFound,
            ),
        ],
        etc_listing: Some(vec!["skills".into()]),
        var_lib_listing: Some(vec!["domovoi-packages".into()]),
        config_files: vec![
            rec("/etc/domovoi/helper.toml", Probe::Open, Access::NotFound),
            rec("/etc/domovoi/agent.toml", Probe::Open, Access::NotFound),
        ],
        run: Some(RunFacts {
            run_entries: vec![],
            systemd_entries: None,
            incoming: None,
        }),
        writes,
        homes: vec![
            rec("/home", Probe::List, Access::Denied),
            rec("/root", Probe::List, Access::NotFound),
        ],
        run_user_exists: false,
        proc_hidden: vec![
            rec("/proc/1234", Probe::Stat, Access::NotFound),
            rec("/proc/1/environ", Probe::Open, Access::NotFound),
        ],
        status: ProcStatus {
            uid: 991,
            gid: 991,
            groups: vec![],
            cap_eff: 0,
            cap_bnd: 0,
            no_new_privs: true,
        },
        mounts: vec![
            MountFact {
                point: p("/etc/domovoi/skills"),
                role: MountRole::SkillsBind,
                read_only: true,
            },
            MountFact {
                point: p("/var/lib/domovoi-packages"),
                role: MountRole::PackagesBind,
                read_only: true,
            },
            MountFact {
                point: p("/var/lib/domovoi-agent"),
                role: MountRole::Mask,
                read_only: true,
            },
            MountFact {
                point: p("/var/lib/domovoi-helper"),
                role: MountRole::Mask,
                read_only: true,
            },
            MountFact {
                point: p("/run"),
                role: MountRole::Mask,
                read_only: true,
            },
        ],
        apparmor_label: None,
        shadow: Some(Access::Denied),
    }
}

fn points(report: &IsolationReport) -> Vec<u8> {
    evaluate_isolation(report).iter().map(|f| f.point).collect()
}

fn status_text(groups: &str, cap_eff: &str, cap_bnd: &str, nnp: u8) -> ProcStatus {
    let text = format!(
        "Name:\trunner\nUid:\t991\t991\t991\t991\nGid:\t991\t991\t991\t991\nGroups:\t{groups}\nCapEff:\t{cap_eff}\nCapBnd:\t{cap_bnd}\nNoNewPrivs:\t{nnp}\n"
    );
    parse_proc_status(&text).expect("parses")
}

// covers: ST-05
// covers: runner_groups_extra
#[test]
fn startup_check_accepts_primary_group_only() {
    assert!(
        evaluate_runner_startup(&status_text("", "0000000000000000", "0000000000000000", 1))
            .is_empty()
    );
    assert!(
        evaluate_runner_startup(&status_text(
            "991",
            "0000000000000000",
            "0000000000000000",
            1
        ))
        .is_empty()
    );
}

#[test]
fn startup_check_rejects_extra_group() {
    let faults = evaluate_runner_startup(&status_text("991 44", "0", "0", 1));
    assert_eq!(faults, [RunnerFault::ExtraGroups { gids: vec![44] }]);
}

// covers: ST-06
// covers: runner_privileges
#[test]
fn startup_check_rejects_nonzero_capabilities() {
    let faults =
        evaluate_runner_startup(&status_text("", "0000000000000001", "000001ffffffffff", 1));
    assert_eq!(
        faults,
        [RunnerFault::Capabilities {
            effective: 1,
            bounding: 0x1ff_ffff_ffff
        }]
    );
}

#[test]
fn startup_check_requires_no_new_privs() {
    let faults = evaluate_runner_startup(&status_text("", "0", "0", 0));
    assert_eq!(faults, [RunnerFault::NoNewPrivsUnset]);
    assert!(matches!(
        parse_proc_status("Uid:\t1\t1\t1\t1\n"),
        Err(RunnerFault::Unreadable { .. })
    ));
}

// covers: ST-13
// covers: runner_not_isolated
#[test]
fn isolation_report_distinguishes_eacces_from_enoent() {
    assert!(evaluate_isolation(&good_report()).is_empty());
    let mut report = good_report();
    report.connects[0].access = Access::Denied;
    report.hidden_state[3].access = Access::Denied;
    report.proc_hidden[1].access = Access::Denied;
    let faults = evaluate_isolation(&report);
    assert_eq!(
        faults.iter().map(|f| f.point).collect::<Vec<_>>(),
        [1, 2, 6]
    );
    assert!(
        faults.iter().all(|f| f.what.contains("mask failed")),
        "{faults:?}"
    );
    report.connects[0].access = Access::Allowed;
    assert!(!evaluate_isolation(&report)[0].what.contains("mask failed"));
    // A report with nothing collected never passes.
    let mut empty = good_report();
    empty.connects.clear();
    assert_eq!(points(&empty), [1]);
}

#[test]
fn isolation_report_accepts_systemd_incoming_and_empty_run_and_rejects_any_other_entry() {
    let mut report = good_report();
    report.run = Some(RunFacts {
        run_entries: vec!["systemd".into()],
        systemd_entries: Some(vec!["incoming".into()]),
        incoming: Some(IncomingFacts {
            is_dir: true,
            uid: 0,
            mode: 0o700,
        }),
    });
    assert!(evaluate_isolation(&report).is_empty());
    for run in [
        RunFacts {
            run_entries: vec!["systemd".into(), "domovoi-helper.sock".into()],
            systemd_entries: None,
            incoming: None,
        },
        RunFacts {
            run_entries: vec!["systemd".into()],
            systemd_entries: Some(vec!["notify".into()]),
            incoming: None,
        },
        RunFacts {
            run_entries: vec!["systemd".into()],
            systemd_entries: Some(vec!["incoming".into()]),
            incoming: Some(IncomingFacts {
                is_dir: true,
                uid: 0,
                mode: 0o755,
            }),
        },
        RunFacts {
            run_entries: vec!["systemd".into()],
            systemd_entries: Some(vec!["incoming".into()]),
            incoming: Some(IncomingFacts {
                is_dir: false,
                uid: 0,
                mode: 0o700,
            }),
        },
        RunFacts {
            run_entries: vec!["systemd".into()],
            systemd_entries: Some(vec!["incoming".into()]),
            incoming: Some(IncomingFacts {
                is_dir: true,
                uid: 1000,
                mode: 0o700,
            }),
        },
    ] {
        report.run = Some(run);
        assert_eq!(points(&report), [3]);
    }
    report.run = None;
    assert_eq!(points(&report), [3]);
    let mut listing = good_report();
    listing.etc_listing = Some(vec!["skills".into(), "hub.toml".into()]);
    assert_eq!(points(&listing), [3]);
}

#[test]
fn isolation_report_accepts_eacces_for_append_to_unowned_file_but_not_success() {
    let mut report = good_report();
    report.writes.push(rec(
        "/var/lib/domovoi-packages/x/f",
        Probe::Append,
        Access::Denied,
    ));
    assert!(evaluate_isolation(&report).is_empty());
    report.writes.push(rec(
        "/var/lib/domovoi-packages/x/g",
        Probe::Append,
        Access::Allowed,
    ));
    assert_eq!(points(&report), [4]);
    let mut other = good_report();
    other.writes.push(rec(
        "/var/lib/domovoi-packages/x/g",
        Probe::Append,
        Access::NotFound,
    ));
    assert_eq!(points(&other), [4]);
    // An answer the probe could not classify (such as EISDIR on a directory) stays a fault;
    // the probe must pick a regular file instead.
    let mut unclassified = good_report();
    unclassified.writes.push(rec(
        "/etc/domovoi/skills/disk_usage_top",
        Probe::Append,
        Access::Other,
    ));
    assert_eq!(points(&unclassified), [4]);
}

#[test]
fn isolation_report_requires_ro_on_both_binds_and_erofs_for_create_rename_unlink() {
    let mut report = good_report();
    report.writes[0].access = Access::Denied;
    report.writes[4].access = Access::Allowed;
    assert_eq!(points(&report), [4, 4]);
    let mut missing = good_report();
    missing.writes.retain(|r| r.how != Probe::Unlink);
    assert_eq!(points(&missing), [4, 4]);

    for index in [0, 1, 2, 3, 4] {
        let mut mounts = good_report();
        mounts.mounts[index].read_only = false;
        assert_eq!(points(&mounts), [7], "mount {index}");
    }
    let mut few = good_report();
    few.mounts.pop();
    assert_eq!(points(&few), [7]);
    let mut home = good_report();
    home.homes[0].access = Access::Allowed;
    home.run_user_exists = true;
    assert_eq!(points(&home), [5, 5]);
    let mut privileged = good_report();
    privileged.status.cap_eff = 1;
    privileged.status.groups = vec![44];
    privileged.status.no_new_privs = false;
    assert_eq!(points(&privileged), [7, 7, 7]);
    // Point 8 is informational.
    let mut shadow = good_report();
    shadow.shadow = Some(Access::Allowed);
    shadow.apparmor_label = Some("unconfined".into());
    assert!(evaluate_isolation(&shadow).is_empty());
}

#[test]
fn check_ids_are_st01_to_st20() {
    assert_eq!(CheckId::ALL.len(), 20);
    for (index, id) in CheckId::ALL.iter().enumerate() {
        assert_eq!(id.as_str(), format!("ST-{:02}", index + 1));
        let json = serde_json::to_string(id).expect("serialises");
        assert_eq!(json, format!("\"{}\"", id.as_str()));
    }
    assert_eq!(CheckId::St05.run_by(), RunBy::Runner);
    assert_eq!(CheckId::St10.run_by(), RunBy::Root);
    assert_eq!(CheckId::St14.run_by(), RunBy::Agent);
}

fn platform() -> PlatformId {
    PlatformId {
        os_id: "debian".into(),
        version_id: "12".into(),
        arch: "x86_64".into(),
    }
}

fn report_with(failing: &[(CheckId, Outcome)]) -> SelftestReport {
    let results = CheckId::ALL
        .iter()
        .map(|id| {
            let outcome = failing
                .iter()
                .find(|(f, _)| f == id)
                .map_or(Outcome::Pass, |(_, o)| o.clone());
            CheckResult::new(*id, outcome)
        })
        .collect();
    SelftestReport {
        results,
        platform: platform(),
        systemd_version: Some("systemd 252".into()),
        boundary: BoundaryState::InEffect,
    }
}

#[test]
fn nd_never_satisfies_a_required_check() {
    for id in REQUIRED_CHECKS {
        let report = report_with(&[(*id, Outcome::nd("not run"))]);
        let decision = classify_platform(&report, &[], WriteOptIn(true));
        assert_eq!(decision.tier, PlatformTier::Blocked, "{id}");
        assert_eq!(decision.blocking, [*id]);
    }
    // An absent result is no better than nd.
    let mut report = report_with(&[]);
    report.results.retain(|r| r.id != CheckId::St13);
    assert_eq!(
        classify_platform(&report, &[], WriteOptIn(false)).blocking,
        [CheckId::St13]
    );
    // ST-08, ST-17, ST-19 and ST-20 are not required.
    let optional = report_with(&[
        (CheckId::St08, Outcome::nd("x")),
        (CheckId::St17, Outcome::nd("not_a_container_install")),
        (CheckId::St19, Outcome::nd("x")),
    ]);
    assert_eq!(
        classify_platform(&optional, &[], WriteOptIn(false)).tier,
        PlatformTier::Untested
    );
}

#[test]
fn untested_platform_allows_skills_with_notice_and_refuses_write_groups_without_opt_in() {
    let decision = classify_platform(&report_with(&[]), SUPPORTED_PLATFORMS, WriteOptIn(false));
    assert_eq!(decision.tier, PlatformTier::Untested);
    assert_eq!(decision.skills, SkillsDecision::AllowWithNotice);
    assert_eq!(decision.helper_write, WriteDecision::NeedsOptIn);
    assert_eq!(decision.helper_read, ReadDecision::Allow);
    assert_eq!(decision.notice, Some(TierNotice::UntestedPlatform));
    assert!(
        decision
            .notice
            .expect("notice")
            .text()
            .contains("not been release-tested")
    );
    assert!(decision.blocking.is_empty());
}

#[test]
fn untested_platform_with_opt_in_allows_write_groups() {
    let decision = classify_platform(&report_with(&[]), &[], WriteOptIn(true));
    assert_eq!(decision.tier, PlatformTier::Untested);
    assert_eq!(decision.helper_write, WriteDecision::Allow);
}

#[test]
fn blocked_platform_refuses_skills_and_write_groups() {
    let fail = Outcome::fail(CheckToken::RunnerNotIsolated, "mask failed");
    let decision = classify_platform(
        &report_with(&[(CheckId::St13, fail)]),
        &[],
        WriteOptIn(true),
    );
    assert_eq!(decision.tier, PlatformTier::Blocked);
    assert_eq!(decision.skills, SkillsDecision::Refuse);
    assert_eq!(decision.helper_write, WriteDecision::Refuse);
    assert_eq!(decision.notice, None);
    let mut no_systemd = report_with(&[]);
    no_systemd.systemd_version = None;
    let decision = classify_platform(&no_systemd, &[], WriteOptIn(true));
    assert_eq!(
        (decision.tier, decision.no_systemd),
        (PlatformTier::Blocked, true)
    );
}

#[test]
fn blocked_platform_still_allows_helper_read_when_its_own_checks_pass() {
    let report = report_with(&[(CheckId::St13, Outcome::nd("runner_probe_unavailable"))]);
    let decision = classify_platform(&report, &[], WriteOptIn(false));
    assert_eq!(decision.tier, PlatformTier::Blocked);
    assert_eq!(decision.helper_read, ReadDecision::Allow);
    for id in [CheckId::St10, CheckId::St11, CheckId::St12, CheckId::St15] {
        let fail = Outcome::fail(CheckToken::PathModeWrong, "x");
        let decision = classify_platform(&report_with(&[(id, fail)]), &[], WriteOptIn(false));
        assert_eq!(decision.helper_read, ReadDecision::Refuse, "{id}");
    }
    let nd = report_with(&[(CheckId::St15, Outcome::nd("needs_agent"))]);
    assert_eq!(
        classify_platform(&nd, &[], WriteOptIn(false)).helper_read,
        ReadDecision::Refuse
    );
}

#[test]
fn supported_requires_a_listed_platform() {
    assert!(SUPPORTED_PLATFORMS.is_empty());
    let listed = [platform()];
    let decision = classify_platform(&report_with(&[]), &listed, WriteOptIn(false));
    assert_eq!(decision.tier, PlatformTier::Supported);
    assert_eq!(decision.skills, SkillsDecision::Allow);
    assert_eq!(decision.helper_write, WriteDecision::Allow);
    assert_eq!(decision.notice, None);
    let other = [PlatformId {
        version_id: "13".into(),
        ..platform()
    }];
    assert_eq!(
        classify_platform(&report_with(&[]), &other, WriteOptIn(false)).tier,
        PlatformTier::Untested
    );
    // Listed but blocked stays blocked.
    let blocked = report_with(&[(
        CheckId::St02,
        Outcome::fail(CheckToken::AgentGroupsUnexpected, "x"),
    )]);
    assert_eq!(
        classify_platform(&blocked, &listed, WriteOptIn(false)).tier,
        PlatformTier::Blocked
    );
}

#[test]
fn warn_never_changes_the_tier() {
    let warn = Outcome::warn(CheckToken::StateWriteSlow, "median 250000 us");
    let report = report_with(&[(CheckId::St20, warn.clone())]);
    assert_eq!(
        classify_platform(&report, &[], WriteOptIn(false)).tier,
        PlatformTier::Untested
    );
    // Even a warning on a required check does not block.
    let on_required = report_with(&[(CheckId::St07, warn)]);
    assert_eq!(
        classify_platform(&on_required, &[], WriteOptIn(false)).tier,
        PlatformTier::Untested
    );
    assert!(!report.any_failed());
}

struct Stub {
    euid: u32,
    groups: Vec<u32>,
    users: Vec<UserEntry>,
}

impl HostInspect for Stub {
    fn euid(&self) -> u32 {
        self.euid
    }
    fn egid(&self) -> u32 {
        self.euid
    }
    fn supplementary_groups(&self) -> Result<Vec<u32>, InspectError> {
        Ok(self.groups.clone())
    }
    fn user(&self, name: &str) -> Result<Option<UserEntry>, InspectError> {
        Ok(self.users.iter().find(|u| u.name == name).cloned())
    }
    fn group(&self, name: &str) -> Result<Option<GroupEntry>, InspectError> {
        Ok((name == JOURNAL_GROUP).then(|| GroupEntry {
            name: name.into(),
            gid: 190,
            members: vec![],
        }))
    }
    fn groups_of(&self, _: &UserEntry) -> Result<Vec<u32>, InspectError> {
        Err(InspectError::Io("unused".into()))
    }
    fn lstat(&self, _: &std::path::Path) -> Result<crate::identity::PathFacts, InspectError> {
        Err(InspectError::NotFound)
    }
    fn open_facts(&self, _: &std::path::Path) -> Result<crate::identity::PathFacts, InspectError> {
        Err(InspectError::NotFound)
    }
    fn list_dir(&self, _: &std::path::Path) -> Result<Vec<String>, InspectError> {
        Err(InspectError::NotFound)
    }
    fn walk_tree(&self, _: &std::path::Path) -> Result<Vec<TreeEntry>, InspectError> {
        Err(InspectError::NotFound)
    }
    fn probe(&self, _: &std::path::Path, _: Probe) -> Access {
        Access::NotFound
    }
    fn read_text(&self, _: &std::path::Path) -> Result<String, InspectError> {
        Err(InspectError::NotFound)
    }
    fn kernel_release(&self) -> Result<String, InspectError> {
        Err(InspectError::NotFound)
    }
    fn state_write_timings(&self, _: &std::path::Path, _: u32) -> Result<Vec<u64>, InspectError> {
        Err(InspectError::NotFound)
    }
}

// covers: ST-01
// covers: privilege_boundary_not_in_effect
// covers: agent_groups_unexpected
#[test]
fn boundary_not_in_effect_for_root_and_for_other_users() {
    let agent = UserEntry {
        name: AGENT_ACCOUNT.into(),
        uid: 990,
        gid: 990,
        shell: "/usr/sbin/nologin".into(),
    };
    let in_effect =
        |euid, groups: &[u32]| evaluate_boundary(euid, Some(&agent), groups, Some(190), Some(999));
    assert_eq!(in_effect(990, &[990, 190]), BoundaryState::InEffect);
    assert!(in_effect(990, &[190]).in_effect());
    assert_eq!(
        in_effect(0, &[]),
        BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentIsRoot
        }
    );
    assert_eq!(
        in_effect(1000, &[]),
        BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentNotDomovoiAgent { uid: 1000 }
        }
    );
    assert_eq!(
        in_effect(990, &[990, 190, 999]),
        BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentGroupsUnexpected { gids: vec![999] }
        }
    );
    assert_eq!(
        evaluate_boundary(990, None, &[], None, None),
        BoundaryState::NotInEffect {
            reason: BoundaryReason::AccountMissing
        }
    );

    let host = |euid, groups: Vec<u32>, users: Vec<UserEntry>| Stub {
        euid,
        groups,
        users,
    };
    assert_eq!(
        inspect_boundary(&host(990, vec![190], vec![agent.clone()])),
        BoundaryState::InEffect
    );
    assert!(!inspect_boundary(&host(0, vec![], vec![agent.clone()])).in_effect());
    assert!(!inspect_boundary(&host(990, vec![190], vec![])).in_effect());
    let json = serde_json::to_string(&BoundaryState::NotInEffect {
        reason: BoundaryReason::AgentIsRoot,
    })
    .expect("serialises");
    assert_eq!(
        json,
        r#"{"boundary":"not_in_effect","reason":"agent_is_root"}"#
    );
}

#[test]
fn selftest_report_round_trips_as_json() {
    let report = report_with(&[(CheckId::St10, Outcome::fail(CheckToken::PathModeWrong, "x"))]);
    let json = serde_json::to_string(&report).expect("serialises");
    let back: SelftestReport = serde_json::from_str(&json).expect("parses");
    assert_eq!(back, report);
    assert!(report.any_failed());
}
