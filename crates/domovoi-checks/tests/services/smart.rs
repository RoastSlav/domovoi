//! SMART counters fire on increase, device conditions fire on state.

use std::sync::Arc;

use domovoi_checks::framework::CheckError;
use domovoi_checks::smart::{SmartHealth, acknowledge};
use domovoi_core::helper::read::{DeviceRef, ReadToken, SmartReportOutput};
use domovoi_core::ports::PrivilegedError;
use domovoi_core::{FactValue, Severity};
use domovoi_testkit::{FakePrivilegedReads, PrivilegedCall, PrivilegedScript};

use crate::harness::{
    Harness, assert_firing, assert_ok, find, firing_targets, number, output, read_fixture, text,
};

const SMARTCTL: &str = "/usr/sbin/smartctl";

fn all(device: &str) -> Vec<&str> {
    vec!["--all", "--json", "--nocheck=standby", device]
}

async fn harness(ata: &str, nvme: &str) -> Harness {
    let h = Harness::new("proc/ports_base").await;
    h.exec.expect(
        SMARTCTL,
        &["--scan-open", "--json"],
        output(&read_fixture("smart/scan.json")),
    );
    set(&h, ata, nvme);
    h
}

fn set(h: &Harness, ata: &str, nvme: &str) {
    h.exec.expect(
        SMARTCTL,
        &all("/dev/sda"),
        output(&read_fixture(&format!("smart/{ata}.json"))),
    );
    h.exec.expect(
        SMARTCTL,
        &all("/dev/nvme0"),
        output(&read_fixture(&format!("smart/{nvme}.json"))),
    );
}

#[tokio::test]
async fn first_run_records_baseline_without_firing() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("ata", "nvme").await;
    let report = h.run(&check).await;
    assert!(firing_targets(&report).is_empty(), "{report:#?}");
    // CRC errors are non-zero on the first run but are not an increase.
    let crc = find(&report, "/dev/sda:udma_crc_error_count");
    assert!(!crc.firing);
    assert_eq!(number(crc, "value"), 2.0);
    assert_ok(&report, "/dev/sda");
    assert_ok(&report, "/dev/nvme0:media_errors");
    assert!(h.state.get("counters").is_some());

    let health = report
        .facts
        .iter()
        .find(|f| f.key == "smart.sda.health")
        .unwrap();
    assert_eq!(health.value, FactValue::Text("PASSED".to_owned()));
    assert!(
        report
            .facts
            .iter()
            .any(|f| f.key == "smart.nvme0.temperature_c")
    );
}

#[tokio::test]
async fn ata_counters_fire_on_increase_not_on_nonzero() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("ata", "nvme").await;
    h.run(&check).await;
    assert!(firing_targets(&h.run(&check).await).is_empty());

    set(&h, "ata_increased", "nvme");
    let report = h.run(&check).await;
    let realloc = find(&report, "/dev/sda:reallocated_sector_ct");
    assert_firing(&report, "/dev/sda:reallocated_sector_ct", Severity::Warning);
    assert_eq!(number(realloc, "value"), 1.0);
    assert_eq!(number(realloc, "previous"), 0.0);
    assert_eq!(number(realloc, "delta"), 1.0);
    assert_eq!(number(realloc, "power_on_hours"), 20000.0);
    assert_eq!(text(realloc, "model"), "WDC WD40EFRX-68N32N0");
    assert_eq!(number(realloc, "temperature_c"), 34.0);
    assert_ok(&report, "/dev/sda:reported_uncorrect");

    // It keeps firing until acknowledged, then stays quiet at the new value.
    assert!(find(&h.run(&check).await, "/dev/sda:reallocated_sector_ct").firing);
    acknowledge(&mut h.state, "/dev/sda", "reallocated_sector_ct", 1);
    assert_ok(&h.run(&check).await, "/dev/sda:reallocated_sector_ct");
}

#[tokio::test]
async fn pending_sector_increase_is_critical() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("ata", "nvme").await;
    h.run(&check).await;
    set(&h, "ata_increased", "nvme");
    let report = h.run(&check).await;
    assert_firing(
        &report,
        "/dev/sda:current_pending_sector",
        Severity::Critical,
    );
    assert_eq!(
        number(find(&report, "/dev/sda:current_pending_sector"), "delta"),
        3.0
    );
}

#[tokio::test]
async fn crc_increase_hints_cable() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("ata", "nvme").await;
    h.run(&check).await;
    set(&h, "ata_increased", "nvme");
    let report = h.run(&check).await;
    let crc = find(&report, "/dev/sda:udma_crc_error_count");
    assert_firing(&report, "/dev/sda:udma_crc_error_count", Severity::Warning);
    assert_eq!(number(crc, "previous"), 2.0);
    assert_eq!(number(crc, "value"), 5.0);
    assert!(text(crc, "hint").contains("cable"));

    // A replaced disk with lower counters re-baselines silently.
    set(&h, "ata", "nvme");
    assert!(firing_targets(&h.run(&check).await).is_empty());
}

#[tokio::test]
async fn nvme_critical_warning_is_critical() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("ata", "nvme_critical").await;
    let report = h.run(&check).await;
    assert_firing(&report, "/dev/nvme0", Severity::Critical);
    assert!(find(&report, "/dev/nvme0").summary.contains("0x04"));
    assert_eq!(text(find(&report, "/dev/nvme0"), "serial"), "S5GXNX0000001");
}

#[tokio::test]
async fn standby_disk_is_skipped() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("standby", "nvme").await;
    let report = h.run(&check).await;
    assert!(report.observations.iter().all(|o| {
        !o.target
            .as_ref()
            .is_some_and(|t| t.as_str().starts_with("/dev/sda"))
    }));
    assert_ok(&report, "/dev/nvme0");
}

const ONE_DISK: &str = r#"{"devices":[{"name":"/dev/sda","protocol":"ATA"}]}"#;

fn port(report: Result<SmartReportOutput, PrivilegedError>) -> Arc<FakePrivilegedReads> {
    Arc::new(FakePrivilegedReads::new(PrivilegedScript {
        scan: Ok(vec![DeviceRef::parse("/dev/sda").unwrap()]),
        report,
        ..PrivilegedScript::default()
    }))
}

#[tokio::test]
async fn smart_check_observations_are_identical_through_the_helper_port() {
    let check = SmartHealth::new().unwrap();
    let json = read_fixture("smart/ata.json");

    let mut direct = Harness::new("proc/ports_base").await;
    direct
        .exec
        .expect(SMARTCTL, &["--scan-open", "--json"], output(ONE_DISK));
    direct
        .exec
        .expect(SMARTCTL, &all("/dev/sda"), output(&json));
    let expected = direct.run(&check).await;

    let mut helped = Harness::new("proc/ports_base").await;
    let fake = port(Ok(SmartReportOutput {
        json,
        exit_code: Some(0),
        truncated: false,
    }));
    helped.privileged = Some(fake.clone());
    let actual = helped.run(&check).await;

    assert_eq!(
        format!("{:?}", actual.observations),
        format!("{:?}", expected.observations)
    );
    assert_eq!(
        format!("{:?}", actual.facts),
        format!("{:?}", expected.facts)
    );
    assert_eq!(
        helped.exec.calls().len(),
        0,
        "no smartctl on the helper path"
    );
    assert_eq!(
        fake.calls(),
        vec![
            PrivilegedCall::SmartScan,
            PrivilegedCall::SmartReport("/dev/sda".to_owned())
        ]
    );
}

#[tokio::test]
async fn smart_check_without_a_privileged_port_runs_the_existing_commands() {
    let check = SmartHealth::new().unwrap();
    let mut h = harness("ata", "nvme").await;
    assert!(h.privileged.is_none());
    let report = h.run(&check).await;
    assert_ok(&report, "/dev/sda");
    assert_eq!(
        h.exec.calls().len(),
        3,
        "one scan and one report per device"
    );
}

#[tokio::test]
async fn smart_check_maps_helper_errors_to_a_check_error() {
    let check = SmartHealth::new().unwrap();
    let mut h = Harness::new("proc/ports_base").await;
    h.privileged = Some(port(Err(PrivilegedError::Failed(ReadToken::ToolMissing))));
    let err = h.try_run(&check).await.unwrap_err();
    assert_eq!(
        err,
        CheckError::Helper("the read failed: tool_missing".to_owned())
    );
    assert!(h.exec.calls().is_empty());

    h.privileged = Some(Arc::new(FakePrivilegedReads::new(PrivilegedScript {
        scan: Err(PrivilegedError::HelperMissing),
        ..PrivilegedScript::default()
    })));
    let err = h.try_run(&check).await.unwrap_err();
    assert!(matches!(err, CheckError::Helper(text) if text.contains("not installed")));
}
