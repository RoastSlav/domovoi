//! Disk, memory, temperature, reboot and reboot-required checks against fixture trees.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_checks::framework::CheckError;
use domovoi_checks::system::{
    CpuTemperature, DiskFree, FsStats, FsUsage, MemoryAvailable, Reboot, RebootRequired,
};
use domovoi_core::ports::FsError;
use domovoi_core::{FactValue, Severity};

use crate::harness::{Harness, assert_firing, assert_ok, find, is_immediate, number, text};

const GIB: u64 = 1024 * 1024 * 1024;

struct FakeStats(BTreeMap<&'static str, (u64, u64)>);

#[async_trait]
impl FsStats for FakeStats {
    async fn usage(&self, mount_point: &str) -> Result<FsUsage, FsError> {
        let (total, free) = self.0.get(mount_point).copied().ok_or(FsError::NotFound)?;
        Ok(FsUsage {
            total_bytes: total * GIB,
            available_bytes: free * GIB,
        })
    }
}

fn disk() -> DiskFree {
    DiskFree::new(Arc::new(FakeStats(BTreeMap::from([
        ("/", (100, 50)),
        ("/srv", (100, 10)),
        ("/mnt/backup disk", (100, 3)),
        ("/proc", (1, 0)),
        ("/run", (1, 0)),
        ("/var/lib/docker/overlay2/abc/merged", (1, 0)),
    ]))))
    .unwrap()
}

#[tokio::test]
async fn disk_free_fires_warning_and_critical_per_mount_and_skips_pseudo_filesystems() {
    let mut h = Harness::new("healthy").await;
    let report = h.run(&disk()).await;
    let targets: Vec<&str> = report
        .observations
        .iter()
        .map(|o| o.target.as_ref().unwrap().as_str())
        .collect();
    assert_eq!(targets, ["/", "/srv", "/mnt/backup disk"]);
    assert_ok(&report, Some("/"));
    assert_firing(&report, Some("/srv"), Severity::Warning);
    assert_firing(&report, Some("/mnt/backup disk"), Severity::Critical);
    let srv = find(&report, Some("/srv"));
    assert_eq!(number(srv, "free_pct"), 10.0);
    assert_eq!(number(srv, "free_gib"), 10.0);
    assert_eq!(text(srv, "device"), "/dev/sdb1");

    let mut h = Harness::new("healthy")
        .await
        .with_thresholds(serde_json::json!({
            "mounts": { "/srv": { "warning_free_pct": 5.0 } },
            "ignore": ["/mnt/backup disk"],
        }));
    let report = h.run(&disk()).await;
    assert_ok(&report, Some("/srv"));
    assert_eq!(report.observations.len(), 2);
}

#[tokio::test]
async fn unknown_threshold_key_is_a_settings_error() {
    let mut h = Harness::new("healthy")
        .await
        .with_thresholds(serde_json::json!({ "warning_free": 10 }));
    match h.try_run(&disk()).await {
        Err(CheckError::Settings(message)) => {
            assert!(message.contains("warning_free"), "{message}")
        }
        other => panic!("expected a settings error, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_fires_below_threshold_and_reports_swap() {
    let mut h = Harness::new("healthy").await;
    let report = h.run(&MemoryAvailable::new().unwrap()).await;
    assert_ok(&report, Some("available"));
    assert_ok(&report, Some("swap"));
    assert_eq!(number(find(&report, Some("swap")), "swap_used_pct"), 25.0);

    let mut h = Harness::new("low_memory").await;
    let report = h.run(&MemoryAvailable::new().unwrap()).await;
    assert_firing(&report, Some("available"), Severity::Warning);
    assert_eq!(
        number(find(&report, Some("available")), "available_gib"),
        1.0
    );
    assert_firing(&report, Some("swap"), Severity::Warning);
    assert_eq!(number(find(&report, Some("swap")), "swap_used_pct"), 60.0);

    let mut h = Harness::new("low_memory")
        .await
        .with_thresholds(serde_json::json!({ "critical_available_gib": 1.2 }));
    let report = h.run(&MemoryAvailable::new().unwrap()).await;
    assert_firing(&report, Some("available"), Severity::Critical);

    let mut h = Harness::new("no_swap").await;
    let report = h.run(&MemoryAvailable::new().unwrap()).await;
    assert_eq!(report.observations.len(), 1);
}

#[tokio::test]
async fn temperature_reads_hwmon_with_labels_and_falls_back_to_thermal_zones() {
    let mut h = Harness::new("hwmon_labels").await;
    let report = h.run(&CpuTemperature::new().unwrap()).await;
    assert_firing(&report, None, Severity::Warning);
    let obs = find(&report, None);
    assert_eq!(number(obs, "max_c"), 90.5);
    assert_eq!(text(obs, "sensor"), "coretemp Core 0");
    assert_eq!(number(obs, "sensors"), 2.0);

    let mut h = Harness::new("thermal_only").await;
    let report = h.run(&CpuTemperature::new().unwrap()).await;
    assert_firing(&report, None, Severity::Critical);
    assert_eq!(text(find(&report, None), "sensor"), "x86_pkg_temp");

    let mut h = Harness::new("desktop").await;
    let report = h.run(&CpuTemperature::new().unwrap()).await;
    assert!(report.observations.is_empty() && report.facts.is_empty());
}

#[tokio::test]
async fn reboot_is_detected_from_boot_id_change_and_not_on_first_run() {
    let check = Reboot::new().unwrap();
    let mut h = Harness::new("healthy").await;
    let first = h.run(&check).await;
    assert!(first.observations.is_empty());
    assert_eq!(
        h.state.get("boot_id"),
        Some("5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f")
    );

    let again = h.run(&check).await;
    assert!(again.observations.is_empty());

    h.state.set("boot_id", "00000000-old-boot");
    let rebooted = h.run(&check).await;
    let obs = find(&rebooted, None);
    assert!(obs.firing);
    assert_eq!(obs.severity, Severity::Info);
    assert!(is_immediate(obs, &check));
    assert_eq!(number(obs, "uptime_s"), 172_800.0);
    assert_eq!(text(obs, "previous_boot_id"), "00000000-old-boot");
    assert_eq!(
        h.state.get("boot_id"),
        Some("5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f")
    );
}

#[tokio::test]
async fn reboot_required_lists_packages() {
    let check = RebootRequired::new().unwrap();
    let mut h = Harness::new("reboot_required").await;
    let report = h.run(&check).await;
    assert_firing(&report, None, Severity::Warning);
    let obs = find(&report, None);
    assert_eq!(number(obs, "package_count"), 12.0);
    let listed: Vec<&str> = text(obs, "packages").split(", ").collect();
    assert_eq!(listed.len(), 10);
    assert_eq!(listed[0], "linux-image-6.12.0");

    let mut h = Harness::new("healthy").await;
    assert_ok(&h.run(&check).await, None);
}

#[tokio::test]
async fn facts_are_emitted_with_units() {
    let mut h = Harness::new("healthy").await;
    let mut facts = h.run(&disk()).await.facts;
    facts.extend(h.run(&MemoryAvailable::new().unwrap()).await.facts);
    facts.extend(h.run(&Reboot::new().unwrap()).await.facts);
    let mut h = Harness::new("hwmon_labels").await;
    facts.extend(h.run(&CpuTemperature::new().unwrap()).await.facts);
    let mut h = Harness::new("on_battery").await;
    facts.extend(
        h.run(&domovoi_checks::system::PowerSupply::new().unwrap())
            .await
            .facts,
    );

    let keys: Vec<&str> = facts.iter().map(|f| f.key.as_str()).collect();
    for expected in [
        "disk./srv.free_gib",
        "memory.available_gib",
        "system.uptime_days",
        "temperature.cpu.max_c",
        "power.BAT0.capacity_pct",
        "power.BAT0.health_pct",
    ] {
        assert!(keys.contains(&expected), "missing {expected} in {keys:?}");
    }
    for fact in &facts {
        assert!(
            fact.unit.as_deref().is_some_and(|u| !u.is_empty()),
            "{fact:?}"
        );
        assert!(matches!(fact.value, FactValue::Number(_)), "{fact:?}");
        assert_eq!(fact.host, h.host);
    }
    let health = facts
        .iter()
        .find(|f| f.key == "power.BAT0.health_pct")
        .unwrap();
    assert_eq!(health.value, FactValue::Number(90.0));
}
