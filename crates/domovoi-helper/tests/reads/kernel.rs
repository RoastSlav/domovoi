use domovoi_core::helper::read::ReadToken;
use domovoi_core::identity::HostDirs;
use domovoi_helper::reads::tools::{ToolEnv, kernel};
use domovoi_testkit::FakeProcessExecutor;
use jiff::Timestamp;

use crate::support::{exited, healthy, ok};

const DMESG: &str = "/usr/bin/dmesg";
const ARGS: [&str; 2] = ["--kernel", "--time-format=iso"];

fn now() -> Timestamp {
    "2026-05-01T12:00:00Z".parse().expect("time")
}

const LOG: &str = "\
2026-05-01T10:00:00,000000+00:00 old line
2026-05-01T11:40:00,123456+00:00 inside window
  continuation of inside
2026-05-01T11:59:00,000000+02:00 offset line, ten hours ago in UTC
2026-05-01T11:50:00,000000+00:00 another inside
";

#[tokio::test]
async fn kernel_log_filters_by_iso_timestamp_and_keeps_continuation_lines() {
    let inspect = healthy(&HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(DMESG, &ARGS, ok(LOG));
    let env = ToolEnv {
        inspect: &inspect,
        exec: &exec,
    };
    let out = kernel::log(env, now(), 30, 500).await.expect("log");
    assert_eq!(
        out.text,
        "2026-05-01T11:40:00,123456+00:00 inside window\n  continuation of inside\n2026-05-01T11:50:00,000000+00:00 another inside"
    );
    let out = kernel::log(env, now(), 1440, 500).await.expect("log");
    assert_eq!(out.text.lines().count(), 5);
}

#[tokio::test]
async fn kernel_log_returns_the_last_n_lines() {
    let inspect = healthy(&HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(DMESG, &ARGS, ok(LOG));
    let env = ToolEnv {
        inspect: &inspect,
        exec: &exec,
    };
    let out = kernel::log(env, now(), 1440, 2).await.expect("log");
    assert_eq!(out.text.lines().count(), 2);
    assert!(out.text.ends_with("another inside"));
}

#[tokio::test]
async fn kernel_log_nonzero_exit_without_output_is_command_failed() {
    let inspect = healthy(&HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(
        DMESG,
        &ARGS,
        exited(
            1,
            "",
            "dmesg: read kernel buffer failed: Operation not permitted",
        ),
    );
    let env = ToolEnv {
        inspect: &inspect,
        exec: &exec,
    };
    let err = kernel::log(env, now(), 5, 5).await.expect_err("fails");
    assert_eq!(err.token, ReadToken::CommandFailed);
    assert_eq!(
        err.detail.as_deref(),
        Some("dmesg: read kernel buffer failed: Operation not permitted")
    );
}
