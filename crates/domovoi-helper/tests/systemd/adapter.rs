//! The `busctl` adapter against recorded replies.

use std::sync::Arc;

use domovoi_core::identity::{PathFacts, PathKind};
use domovoi_core::ports::ProcessOutput;
use domovoi_core::unit::{SystemdView, UnitId, UnitName};
use domovoi_helper::systemd::{BusctlView, dropin_dir_names};
use domovoi_testkit::{FakeHostInspect, FakeProcessExecutor};

const LOAD: &str = include_str!("../fixtures/busctl/load_unit.json");
const UNIT: &str = include_str!("../fixtures/busctl/unit_all.json");
const SERVICE: &str = include_str!("../fixtures/busctl/service_all.json");
const PATH: &str = "/org/freedesktop/systemd1/unit/example_2eservice";

#[cfg(test)]
fn ok(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        ..ProcessOutput::default()
    }
}

#[cfg(test)]
fn view() -> BusctlView {
    let exec = FakeProcessExecutor::new();
    exec.expect(
        "/usr/bin/busctl",
        &[
            "--json=short",
            "call",
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "LoadUnit",
            "s",
            "example.service",
        ],
        ok(LOAD),
    );
    for (interface, reply) in [
        ("org.freedesktop.systemd1.Unit", UNIT),
        ("org.freedesktop.systemd1.Service", SERVICE),
    ] {
        exec.expect(
            "/usr/bin/busctl",
            &[
                "--json=short",
                "call",
                "org.freedesktop.systemd1",
                PATH,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                "s",
                interface,
            ],
            ok(reply),
        );
    }
    let inspect = FakeHostInspect::new().ids(0, 0).path(
        "/usr/bin/busctl",
        PathFacts {
            kind: Some(PathKind::File),
            symlink: false,
            mode: 0o755,
            uid: 0,
            gid: 0,
        },
    );
    BusctlView::new(Arc::new(exec), Arc::new(inspect))
}

#[tokio::test]
async fn plan_time_reads_an_inactive_unit_with_load_unit() {
    let props = view()
        .load_unit(&UnitName::parse("example.service").unwrap())
        .await
        .expect("reads");
    assert_eq!(props.id, "example.service");
    assert!(props.names.contains("alias.service"));
    assert_eq!(props.load_state, "loaded");
    assert_eq!(props.unit_file_state, "disabled");
    assert!(!props.transient && !props.need_daemon_reload);
    assert!(props.fragment_path.ends_with("/example.service"));
    assert!(props.drop_in_paths.is_empty());
    assert!(props.triggered_by.contains("example.socket"));
}

#[tokio::test]
async fn loaded_properties_come_from_arrays_and_carry_no_runtime_fields() {
    let loaded = view()
        .service_props(&UnitId::parse("example.service").unwrap())
        .await
        .expect("reads");
    assert_eq!(loaded.exec_start.len(), 1);
    assert_eq!(loaded.exec_start[0].argv[1], "--scope");
    assert!(!loaded.exec_start[0].ignore_failure);
    assert_eq!(loaded.service_type, "notify-reload");
    assert_eq!(loaded.start_limit_burst, 5);
    assert_eq!(loaded.start_limit_interval_usec, 10_000_000);
    assert!(loaded.requires.contains("example.socket"));
}

#[tokio::test]
async fn runtime_reads_the_invocation_as_hex_and_the_result() {
    let runtime = view()
        .runtime(&UnitId::parse("example.service").unwrap())
        .await
        .expect("reads");
    assert_eq!(runtime.invocation_id.len(), 32);
    assert!(runtime.invocation_id.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(runtime.active_state, "active");
    assert_eq!(runtime.result, "success");
    assert_eq!(runtime.main_pid, 914);
}

#[tokio::test]
async fn a_missing_or_untrusted_busctl_is_a_read_failure_not_a_default() {
    let exec = FakeProcessExecutor::new();
    let view = BusctlView::new(Arc::new(exec), Arc::new(FakeHostInspect::new().ids(0, 0)));
    let err = view
        .load_unit(&UnitName::parse("example.service").unwrap())
        .await
        .expect_err("no busctl");
    assert!(err.0.contains("busctl"));
}

#[test]
fn dropin_directories_cover_the_name_and_its_dash_prefixes() {
    let id = UnitId::parse("a-b-c.service").unwrap();
    let names = ["a-b-c.service".to_owned(), "alias.service".to_owned()].into();
    let dirs = dropin_dir_names(&id, &names);
    for expected in [
        "a-b-c.service.d",
        "alias.service.d",
        "a-.service.d",
        "a-b-.service.d",
    ] {
        assert!(dirs.contains(expected), "{expected} in {dirs:?}");
    }
}
