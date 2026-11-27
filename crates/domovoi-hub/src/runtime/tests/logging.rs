//! What the hub writes to its log at start: the banner names build, config and addresses.

use domovoi_testkit::log_capture::LogCapture;

use super::at;
use super::harness::Harness;

#[tokio::test]
async fn the_start_up_banner_says_what_build_and_configuration_runs() {
    let harness = Harness::new("", at("2026-01-01T00:00:00Z")).await;
    let (logs, _guard) = LogCapture::start();
    harness.runtime.as_ref().unwrap().log_banner("abc1234");
    let lines = logs.lines_with("domovoi hub starting");
    assert_eq!(lines.len(), 1, "exactly one banner line: {}", logs.text());
    let line = &lines[0];
    for field in [
        "INFO",
        "version=",
        "commit=abc1234",
        "config_file=none",
        "data_dir=",
        "web_bind=",
        "agent_bind=",
        "public_url=",
        "tls_fingerprint=",
        "settings_revision=0",
        "settings_from_file_or_env=none",
        "user=",
    ] {
        assert!(line.contains(field), "banner lacks {field}: {line}");
    }
}

#[tokio::test]
async fn a_finished_backup_logs_its_path_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("domovoi-20261127T000000Z.db");
    std::fs::write(&file, vec![0u8; 321]).unwrap();
    let store = domovoi_testkit::InMemoryStore::new();
    let (logs, _guard) = LogCapture::start();
    crate::runtime::backup_job::record_backup_result(
        &store,
        &crate::PatternRedactor::default(),
        at("2026-11-27T00:00:00Z"),
        Ok(file.clone()),
    )
    .await;
    let line = logs.lines_with("nightly backup written").remove(0);
    assert!(line.contains(&file.display().to_string()), "{line}");
    assert!(line.contains("size_bytes=321"), "{line}");
}

#[tokio::test]
async fn the_backup_audit_row_carries_the_file_name_and_size() {
    use domovoi_core::ports::{AuditFilter, AuditLog, Page};

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("domovoi-20261127T000000Z.db");
    std::fs::write(&file, vec![0u8; 321]).unwrap();
    let store = domovoi_testkit::InMemoryStore::new();
    crate::runtime::backup_job::audit_backup(&store, at("2026-11-27T00:00:00Z"), &Ok(file)).await;
    let rows = store
        .list(&AuditFilter::default(), &Page::first(10))
        .await
        .unwrap()
        .items;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "backup.created");
    assert_eq!(rows[0].detail["file"], "domovoi-20261127T000000Z.db");
    assert_eq!(rows[0].detail["size_bytes"], 321);
}
