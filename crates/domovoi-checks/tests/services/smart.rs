//! SMART counters fire on increase, device conditions fire on state.

use domovoi_checks::smart::{SmartHealth, acknowledge};
use domovoi_core::{FactValue, Severity};

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
