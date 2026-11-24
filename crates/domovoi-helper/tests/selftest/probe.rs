//! The isolation probe: the eight points collected through the inspection port.

use domovoi_core::identity::HostDirs;
use domovoi_core::isolation::evaluate_isolation;
use domovoi_core::ports::Access;
use domovoi_helper::probe::{ProbeArgs, collect};
use domovoi_testkit::FakeHostInspect;

use super::support::{HELPER_PID, healthy_host};

const STATUS: &str = "Uid:\t991\t991\t991\t991\nGid:\t991\t991\t991\t991\nGroups:\t991\nCapEff:\t0000000000000000\nCapBnd:\t0000000000000000\nNoNewPrivs:\t1\n";

fn masked_host() -> FakeHostInspect {
    let dirs = HostDirs::default();
    healthy_host()
        .any_access(&dirs.helper_socket, Access::NotFound)
        .any_access("/run/domovoi-skill-runner.sock", Access::NotFound)
        .any_access(&dirs.state_dir, Access::NotFound)
        .any_access(&dirs.helper_dir, Access::NotFound)
        .any_access(dirs.helper_dir.join("attest.key"), Access::NotFound)
        .any_access(dirs.etc_dir.join("helper.toml"), Access::NotFound)
        .any_access(dirs.etc_dir.join("agent.toml"), Access::NotFound)
        .any_access("/proc/1/environ", Access::NotFound)
        .any_access(format!("/proc/{HELPER_PID}"), Access::NotFound)
        .text("/proc/self/status", STATUS)
        .text("/proc/self/mountinfo", "")
}

#[test]
fn probe_collect_fills_every_point_from_the_inspect_port() {
    let host = masked_host();
    let report = collect(
        &host,
        &ProbeArgs {
            helper_pid: HELPER_PID,
        },
    );
    assert_eq!(report.connects.len(), 2);
    assert_eq!(report.hidden_state.len(), 6);
    assert_eq!(report.config_files.len(), 2);
    assert_eq!(report.homes.len(), 2);
    assert_eq!(report.proc_hidden.len(), 2);
    assert!(report.shadow.is_some());
    assert!(
        host.calls()
            .iter()
            .any(|c| format!("{c:?}").contains("mountinfo")),
        "the mounts come from /proc/self/mountinfo"
    );
}

#[test]
fn probe_collect_reports_a_failed_mask_as_a_fault() {
    let dirs = HostDirs::default();
    let host = masked_host().any_access(dirs.helper_dir.join("attest.key"), Access::Denied);
    let report = collect(
        &host,
        &ProbeArgs {
            helper_pid: HELPER_PID,
        },
    );
    let faults = evaluate_isolation(&report);
    assert!(faults.iter().any(|f| f.point == 2), "{faults:?}");
}
