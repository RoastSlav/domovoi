//! ST-07 and ST-09: unit properties through an instance name, durations and unit text.

use domovoi_core::identity::CheckToken;
use domovoi_core::isolation::{CheckId, Outcome};
use domovoi_core::ports::ProcessOutput;
use domovoi_helper::selftest::{
    SYSTEMCTL, SystemdDuration, UnitTextFault, inspect_units, parse_systemd_duration,
    scan_unit_text,
};
use domovoi_testkit::FakeProcessExecutor;

use super::support::*;

#[test]
fn selftest_parses_systemd_durations_6s_1min_infinity_and_composites() {
    assert_eq!(
        parse_systemd_duration("6s"),
        Some(SystemdDuration::from_secs(6))
    );
    assert_eq!(
        parse_systemd_duration("1min"),
        Some(SystemdDuration::from_secs(60))
    );
    assert_eq!(
        parse_systemd_duration("1min 30s"),
        Some(SystemdDuration::from_secs(90))
    );
    assert_eq!(
        parse_systemd_duration("infinity"),
        Some(SystemdDuration::Infinity)
    );
    assert_eq!(parse_systemd_duration("soon"), None);
}

#[tokio::test]
async fn selftest_reads_unit_properties_through_an_instance_name_and_runtime_max_usec() {
    let exec = healthy_executor();
    let found = inspect_units(&exec, SYSTEMCTL).await;
    assert!(
        found
            .iter()
            .all(|u| u.modified.is_empty() && u.not_inspectable.is_empty())
    );
    let queried: Vec<String> = programs(&exec)
        .into_iter()
        .filter_map(|c| c.args.last().cloned())
        .collect();
    assert!(queried.contains(&format!("domovoi-helper@{INSTANCE}.service")));
    assert!(!queried.iter().any(|u| u == "domovoi-helper@.service"));
    let service_call = programs(&exec)
        .into_iter()
        .find(|c| c.args.last().is_some_and(|u| u.contains('@')))
        .expect("service queried");
    assert!(
        service_call
            .args
            .iter()
            .any(|a| a.contains("RuntimeMaxUSec"))
    );
}

// covers: ST-07, unit_not_inspectable
#[tokio::test]
async fn selftest_treats_a_failing_systemctl_show_as_unit_not_inspectable() {
    let exec = FakeProcessExecutor::new();
    exec.expect(
        SYSTEMCTL,
        &["--version"],
        ok_output("systemd 255 (255.4)\n"),
    );
    let property =
        format!("--property=LoadState,FragmentPath,DropInPaths,NeedDaemonReload,{SOCKET_PROPS}");
    exec.expect(
        SYSTEMCTL,
        &["show", "--no-pager", &property, SOCKET_UNIT],
        ProcessOutput {
            exit_code: Some(1),
            ..ProcessOutput::default()
        },
    );
    let found = inspect_units(&exec, SYSTEMCTL).await;
    let socket = found
        .iter()
        .find(|u| u.unit == SOCKET_UNIT)
        .expect("socket");
    assert!(!socket.not_inspectable.is_empty());
    let result = domovoi_helper::selftest::st07(&found);
    assert_eq!(result.id, CheckId::St07);
    assert!(matches!(
        result.outcome,
        Outcome::Fail {
            token: CheckToken::UnitNotInspectable,
            ..
        }
    ));
}

// covers: unit_modified
#[tokio::test]
async fn selftest_flags_a_dropin_as_unit_modified() {
    let socket = socket_show().replace(
        "DropInPaths=",
        "DropInPaths=/etc/systemd/system/domovoi-helper.socket.d/x.conf",
    );
    let exec = executor_with(&socket, &service_show(), &agent_show());
    let found = inspect_units(&exec, SYSTEMCTL).await;
    let result = domovoi_helper::selftest::st07(&found);
    assert!(matches!(
        result.outcome,
        Outcome::Fail {
            token: CheckToken::UnitModified,
            ..
        }
    ));
}

#[tokio::test]
async fn selftest_flags_a_pending_reload_as_unit_modified() {
    let service = service_show().replace("NeedDaemonReload=no", "NeedDaemonReload=yes");
    let exec = executor_with(&socket_show(), &service, &agent_show());
    let found = inspect_units(&exec, SYSTEMCTL).await;
    assert!(matches!(
        domovoi_helper::selftest::st07(&found).outcome,
        Outcome::Fail {
            token: CheckToken::UnitModified,
            ..
        }
    ));
}

// covers: unit_exec_under_run
#[test]
fn selftest_flags_exec_start_under_run() {
    for path in ["/run/domovoi", "/tmp/x", "/dev/shm/x"] {
        let faults = scan_unit_text(&format!("[Service]\nExecStart={path} helper serve\n"));
        assert!(matches!(
            faults.as_slice(),
            [UnitTextFault::ExecUnderRun { .. }]
        ));
    }
    assert!(
        scan_unit_text("[Service]\nExecStart=/usr/local/bin/domovoi helper serve\n").is_empty()
    );
}

// covers: unit_hardcoded_id
#[test]
fn selftest_flags_hardcoded_numeric_ids_in_units() {
    let faults =
        scan_unit_text("[Service]\nUser=990\nGroup=domovoi-agent\nSupplementaryGroups=12\n");
    assert_eq!(faults.len(), 2);
    assert!(scan_unit_text("[Service]\nUser=domovoi-agent\n").is_empty());
}
