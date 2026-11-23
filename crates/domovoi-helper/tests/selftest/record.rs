//! The platform record: written atomically under the lock, read by the helper for `Groups`.

use std::os::unix::fs::PermissionsExt;

use domovoi_core::identity::HostDirs;
use domovoi_core::isolation::{
    PlatformRecordState, PlatformTier, SelftestReport, TierDecision, WriteOptIn,
};
use domovoi_helper::platform::record_platform;
use domovoi_helper::selftest::{Mode, decide, run_selftest};
use domovoi_helper::serve::read_platform_record;
use domovoi_helper::state::{StateLayout, init};
use domovoi_testkit::FakeClock;
use jiff::Timestamp;

use super::support::*;

fn current_ids() -> StateLayout {
    StateLayout {
        root_uid: nix::unistd::geteuid().as_raw(),
        root_gid: nix::unistd::getegid().as_raw(),
    }
}

async fn report_and_decision() -> (SelftestReport, TierDecision) {
    let host = healthy_host();
    let exec = healthy_executor();
    expect_child(&exec, &child_json());
    let d = dirs();
    let health = FakeHealth::ok();
    let report = run_selftest(&env(&host, &exec, &health, &d), Mode::Installer, None)
        .await
        .expect("report");
    let decision = decide(&report, WriteOptIn(false));
    (report, decision)
}

fn clock() -> FakeClock {
    FakeClock::new("2026-03-01T10:00:00Z".parse::<Timestamp>().expect("time"))
}

#[tokio::test]
async fn selftest_record_writes_platform_json_atomically_under_the_lock() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("helper");
    init(current_ids(), &dir).expect("init");
    let (report, decision) = report_and_decision().await;
    let record =
        record_platform(&healthy_host(), &clock(), &dir, &report, &decision).expect("record");
    assert_eq!(record.kernel_release, "6.8.0-test");
    assert_eq!(record.tier, decision.tier);
    let file = dir.join("platform.json");
    let mode = std::fs::metadata(&file).expect("file").permissions().mode() & 0o7777;
    assert_eq!(mode, 0o600);
    let text = std::fs::read_to_string(&file).expect("read");
    assert!(
        text.contains("2026-03-01T10:00:00Z"),
        "recorded_at comes from the clock: {text}"
    );
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .expect("list")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn helper_groups_report_platform_record_missing_and_stale() {
    let (report, decision) = report_and_decision().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("helper");
    init(current_ids(), &dir).expect("init");
    let dirs = HostDirs {
        helper_dir: dir.clone(),
        ..HostDirs::default()
    };
    // No record on disk.
    let (state, record) = read_platform_record(&healthy_host(), &dirs);
    assert_eq!(state, PlatformRecordState::Missing);
    assert!(record.is_none());
    // Written on this kernel.
    record_platform(&healthy_host(), &clock(), &dir, &report, &decision).expect("record");
    let text = std::fs::read_to_string(dir.join("platform.json")).expect("read");
    let host_with_file = healthy_host().text(dir.join("platform.json"), text);
    let (state, record) = read_platform_record(&host_with_file, &dirs);
    assert_eq!(state, PlatformRecordState::Ok);
    assert_eq!(record.map(|r| r.tier), Some(decision.tier));
    // The kernel changed since.
    let upgraded = host_with_file.kernel_release_text("6.9.1-newer");
    let (state, record) = read_platform_record(&upgraded, &dirs);
    assert_eq!(state, PlatformRecordState::Stale);
    assert!(record.is_none());
    assert_ne!(
        decision.tier,
        PlatformTier::Supported,
        "nothing is release-tested"
    );
}
