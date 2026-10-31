//! Kernel log classification with the journal cursor, and power supply rules.

use domovoi_checks::system::{KernelErrors, PowerSupply};
use domovoi_core::Severity;

use crate::harness::{
    Harness, args, assert_firing, assert_ok, find, is_immediate, number, output, text,
};

const JOURNAL: &str = "/usr/bin/journalctl";
const FIRST_RUN: &[&str] = &[
    "--no-pager",
    "--quiet",
    "--output=cat",
    "--show-cursor",
    "--since=-15min",
    "--dmesg",
];

const KERNEL_LOG: &str = "\
nvme nvme0: I/O 12 QID 3 timeout, reset controller
blk_update_request: I/O error, dev sda, sector 1234 op 0x0:(READ)
Buffer I/O error on dev sda1, logical block 5
Out of memory: Killed process 4321 (java) total-vm:812345kB
pcieport 0000:00:1c.0: PCIe Bus Error: severity=Corrected, type=Physical Layer
pcieport 0000:00:1c.0: PCIe Bus Error: severity=Corrected, type=Physical Layer
pcieport 0000:00:1c.0: PCIe Bus Error: severity=Uncorrected (Fatal), type=Transaction Layer
e1000e 0000:00:1f.6 eth0: NIC Link is Up 1000 Mbps Full Duplex
-- cursor: s=abc;i=1f
";

#[tokio::test]
async fn kernel_errors_classify_patterns_and_keep_counts_out_of_targets() {
    let check = KernelErrors::new().unwrap();
    let mut h = Harness::new("kernel").await;
    h.exec.expect(JOURNAL, FIRST_RUN, output(KERNEL_LOG));
    let report = h.run(&check).await;

    let mut targets: Vec<&str> = report
        .observations
        .iter()
        .map(|o| o.target.as_ref().unwrap().as_str())
        .collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        [
            "io_error",
            "nvme_reset",
            "oom_kill",
            "pcie_corrected",
            "pcie_uncorrected"
        ]
    );
    assert_firing(&report, Some("io_error"), Severity::Warning);
    assert_firing(&report, Some("oom_kill"), Severity::Warning);
    assert_firing(&report, Some("nvme_reset"), Severity::Critical);
    assert_firing(&report, Some("pcie_uncorrected"), Severity::Critical);
    assert_firing(&report, Some("pcie_corrected"), Severity::Info);

    let io = find(&report, Some("io_error"));
    assert_eq!(number(io, "count"), 2.0);
    assert_eq!(text(io, "device"), "sda");
    assert_eq!(text(io, "examples").lines().count(), 2);
    assert_eq!(text(find(&report, Some("oom_kill")), "process"), "java");
    assert_eq!(number(find(&report, Some("pcie_corrected")), "count"), 2.0);
    for obs in &report.observations {
        let target = obs.target.as_ref().unwrap().as_str();
        assert!(
            !target.chars().any(|c| c.is_ascii_digit()),
            "volatile value in target {target}"
        );
    }
}

#[tokio::test]
async fn kernel_errors_advance_cursor_and_do_not_repeat_lines() {
    let check = KernelErrors::new().unwrap();
    let mut h = Harness::new("kernel").await;
    h.exec.expect(JOURNAL, FIRST_RUN, output(KERNEL_LOG));
    let first = h.run(&check).await;
    assert_eq!(first.observations.len(), 5);
    assert_eq!(h.state.get("cursor"), Some("s=abc;i=1f"));

    let mut second_args: Vec<&str> = FIRST_RUN.to_vec();
    second_args[4] = "--after-cursor=s=abc;i=1f";
    h.exec.expect(
        JOURNAL,
        &second_args,
        output("e1000e eth0: NIC Link is Down\n-- cursor: s=abc;i=20\n"),
    );
    let second = h.run(&check).await;
    assert!(second.observations.is_empty());
    assert_eq!(h.state.get("cursor"), Some("s=abc;i=20"));

    let calls = h.exec.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(args(&calls[1]), second_args);

    // Output without a cursor line keeps the old cursor.
    second_args[4] = "--after-cursor=s=abc;i=20";
    h.exec.expect(JOURNAL, &second_args, output(""));
    h.run(&check).await;
    assert_eq!(h.state.get("cursor"), Some("s=abc;i=20"));
}

#[tokio::test]
async fn kernel_errors_report_a_missing_journalctl() {
    let mut h = Harness::new("healthy").await;
    assert!(h.try_run(&KernelErrors::new().unwrap()).await.is_err());
}

#[tokio::test]
async fn power_on_battery_is_immediate_critical() {
    let check = PowerSupply::new().unwrap();
    let mut h = Harness::new("on_battery").await;
    let report = h.run(&check).await;
    assert_firing(&report, Some("ac"), Severity::Critical);
    assert!(is_immediate(find(&report, Some("ac")), &check));
    assert_ok(&report, Some("BAT0"));
    assert_eq!(number(find(&report, Some("BAT0")), "health_pct"), 90.0);
}

#[tokio::test]
async fn low_battery_while_discharging_is_critical() {
    let check = PowerSupply::new().unwrap();
    let mut h = Harness::new("low_battery").await;
    let report = h.run(&check).await;
    assert_firing(&report, Some("BAT0"), Severity::Critical);
    assert_eq!(number(find(&report, Some("BAT0")), "capacity_pct"), 20.0);
    assert!(!is_immediate(find(&report, Some("BAT0")), &check));
}

#[tokio::test]
async fn charge_limit_not_applied_warns() {
    let check = PowerSupply::new().unwrap();
    let mut h = Harness::new("charge_limit_off").await;
    let report = h.run(&check).await;
    assert_ok(&report, Some("ac"));
    assert!(
        report
            .observations
            .iter()
            .all(|o| o.target.as_ref().unwrap().as_str() != "BAT0.charge_limit")
    );

    let mut h = Harness::new("charge_limit_off")
        .await
        .with_thresholds(serde_json::json!({ "charge_limit_pct": 80 }));
    let report = h.run(&check).await;
    assert_firing(&report, Some("BAT0.charge_limit"), Severity::Warning);
    assert_eq!(
        number(find(&report, Some("BAT0.charge_limit")), "applied_pct"),
        100.0
    );

    let mut h = Harness::new("charge_limit_off")
        .await
        .with_thresholds(serde_json::json!({ "charge_limit_pct": 100 }));
    assert_ok(&h.run(&check).await, Some("BAT0.charge_limit"));
}

#[tokio::test]
async fn desktop_without_battery_reports_nothing() {
    let check = PowerSupply::new().unwrap();
    for scenario in ["desktop", "healthy"] {
        let mut h = Harness::new(scenario).await;
        let report = h.run(&check).await;
        assert!(report.observations.is_empty(), "{scenario}: {report:#?}");
        assert!(report.facts.is_empty());
    }
}
