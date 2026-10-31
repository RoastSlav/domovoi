//! Failed and watched systemd units.

use domovoi_checks::units::{
    FailedUnits, WatchedUnits, parse_failed_json, parse_failed_plain, parse_show,
};
use domovoi_core::ports::ProcessOutput;
use domovoi_core::{Criticality, Severity};
use jiff::SignedDuration;

use crate::harness::{
    Harness, assert_firing, assert_ok, find, firing_targets, number, output, read_fixture, text,
};

const SYSTEMCTL: &str = "/usr/bin/systemctl";
const FAILED: &[&str] = &[
    "list-units",
    "--state=failed",
    "--no-legend",
    "--plain",
    "--no-pager",
];
const FAILED_JSON: &[&str] = &[
    "list-units",
    "--state=failed",
    "--no-legend",
    "--plain",
    "--no-pager",
    "--output=json",
];
const SHOW: &[&str] = &[
    "show",
    "--no-pager",
    "--property=ActiveState,SubState,NRestarts,Result",
    "--",
    "docker.service",
];

#[tokio::test]
async fn failed_units_parse_json_and_plain_output() {
    let json = parse_failed_json(&read_fixture("systemd/failed.json")).unwrap();
    let plain = parse_failed_plain(&read_fixture("systemd/failed_plain.txt"));
    assert_eq!(json, plain);
    assert_eq!(json[1].unit, "certbot.timer");
    assert_eq!(json[1].description, "Renew certificates");

    let check = FailedUnits::new().unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.exec.expect(
        SYSTEMCTL,
        FAILED_JSON,
        output(&read_fixture("systemd/failed.json")),
    );
    let report = h.run(&check).await;
    assert_eq!(firing_targets(&report), ["backup.service", "certbot.timer"]);
    assert_firing(&report, "backup.service", Severity::Warning);
    assert_eq!(
        text(find(&report, "backup.service"), "description"),
        "Nightly backup"
    );

    // Older systemd rejects --output=json; the plain listing is used instead.
    let mut h = Harness::new("proc/ports_base").await;
    h.exec.expect(
        SYSTEMCTL,
        FAILED_JSON,
        ProcessOutput {
            exit_code: Some(1),
            stderr: "Unknown output 'json'.".to_owned(),
            ..ProcessOutput::default()
        },
    );
    h.exec.expect(
        SYSTEMCTL,
        FAILED,
        output(&read_fixture("systemd/failed_plain.txt")),
    );
    let report = h.run(&check).await;
    assert_eq!(firing_targets(&report), ["backup.service", "certbot.timer"]);

    // Units that recover resolve.
    h.exec.expect(SYSTEMCTL, FAILED_JSON, output("[]"));
    let recovered = h.run(&check).await;
    assert!(firing_targets(&recovered).is_empty());
    assert_ok(&recovered, "backup.service");
    assert_ok(&recovered, "certbot.timer");
}

fn show(active: &str, restarts: u32) -> ProcessOutput {
    output(&format!(
        "ActiveState={active}\nSubState=running\nNRestarts={restarts}\nResult=success\n"
    ))
}

#[tokio::test]
async fn watched_unit_restart_loop() {
    let check = WatchedUnits::new().unwrap();
    let mut h = Harness::new("proc/ports_base")
        .await
        .with_thresholds(serde_json::json!({ "units": ["docker.service"] }));
    h.exec.expect(SYSTEMCTL, SHOW, show("active", 1));
    let first = h.run(&check).await;
    assert_ok(&first, "docker.service");
    assert_ok(&first, "docker.service:restarts");

    h.advance(SignedDuration::from_mins(10));
    h.exec.expect(SYSTEMCTL, SHOW, show("active", 4));
    let looping = h.run(&check).await;
    assert_ok(&looping, "docker.service");
    assert_firing(&looping, "docker.service:restarts", Severity::Warning);
    assert_eq!(
        number(find(&looping, "docker.service:restarts"), "restarts"),
        3.0
    );
}

#[tokio::test]
async fn watched_unit_not_active_uses_criticality() {
    let check = WatchedUnits::new().unwrap();
    let mut h = Harness::new("proc/ports_base").await.with_thresholds(
        serde_json::json!({ "units": ["docker.service"], "critical": ["docker.service"] }),
    );
    h.exec.expect(SYSTEMCTL, SHOW, show("failed", 0));
    assert_firing(&h.run(&check).await, "docker.service", Severity::Critical);

    let mut h = Harness::new("proc/ports_base")
        .await
        .with_thresholds(serde_json::json!({ "units": ["docker.service"] }));
    h.exec.expect(SYSTEMCTL, SHOW, show("inactive", 0));
    assert_firing(&h.run(&check).await, "docker.service", Severity::Warning);
    h.settings.criticality = Some(Criticality::Critical);
    assert_firing(&h.run(&check).await, "docker.service", Severity::Critical);

    let mut h = Harness::new("proc/ports_base")
        .await
        .with_thresholds(serde_json::json!({ "units": ["-bad unit"] }));
    assert!(h.try_run(&check).await.is_err());

    let props = parse_show("ActiveState=active\nNRestarts=x\n");
    assert_eq!(props.active, "active");
    assert_eq!(props.restarts, None);
}
