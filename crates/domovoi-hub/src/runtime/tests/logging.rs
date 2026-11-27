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
