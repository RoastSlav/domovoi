use domovoi_config::parse_helper;
use domovoi_core::helper::read::{DeviceRef, MAX_DEVICES, ReadToken};
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse};
use domovoi_core::identity::PathKind;
use domovoi_core::ports::{ProcessError, ProcessOutput};
use domovoi_helper::reads::tools::{
    self, Tool, ToolEnv, kernel_args, net_args, smart_glob, smart_report_args, smart_scan_args,
};
use domovoi_testkit::{FakeHostInspect, FakeProcessExecutor};

use crate::support::{ask, clock, context, exited, facts, healthy, ok, tool_facts};

const SMARTCTL: &str = "/usr/sbin/smartctl";

fn patterns() -> Vec<String> {
    parse_helper("")
        .expect("loads")
        .0
        .smart_device_patterns()
        .to_vec()
}

fn dev(text: &str) -> DeviceRef {
    DeviceRef::parse(text).expect("device")
}

fn env<'a>(inspect: &'a FakeHostInspect, exec: &'a FakeProcessExecutor) -> ToolEnv<'a> {
    ToolEnv { inspect, exec }
}

#[test]
fn argv_templates_match_the_brief() {
    let report = smart_report_args(&dev("/dev/sda"));
    insta::assert_debug_snapshot!(
        "argv_templates",
        (smart_scan_args(), report, kernel_args(), net_args())
    );
    let spec = tools::spec(SMARTCTL, smart_scan_args());
    assert!(spec.env.is_empty());
    assert_eq!(spec.cwd, None);
    assert_eq!(spec.stdin, None);
    assert_eq!(spec.run_as, None);
    assert_eq!(spec.timeout.as_secs(), 15);
    assert_eq!(spec.max_output_bytes, 2 * 1024 * 1024);
}

#[test]
fn tool_lookup_uses_fixed_candidates_and_rejects_a_writable_or_non_root_binary() {
    let none = FakeHostInspect::new();
    assert_eq!(tools::locate(&none, Tool::Smartctl), None);
    let later = FakeHostInspect::new().path("/sbin/smartctl", tool_facts());
    assert_eq!(
        tools::locate(&later, Tool::Smartctl),
        Some("/sbin/smartctl")
    );
    let first = FakeHostInspect::new()
        .path("/usr/sbin/smartctl", tool_facts())
        .path("/sbin/smartctl", tool_facts());
    assert_eq!(tools::locate(&first, Tool::Smartctl), Some(SMARTCTL));
    for bad in [
        facts(PathKind::File, 0o775, 0),
        facts(PathKind::File, 0o757, 0),
        facts(PathKind::File, 0o755, 1000),
        facts(PathKind::Dir, 0o755, 0),
        domovoi_core::identity::PathFacts {
            symlink: true,
            ..tool_facts()
        },
    ] {
        let host = FakeHostInspect::new().path(SMARTCTL, bad);
        assert_eq!(tools::locate(&host, Tool::Smartctl), None);
    }
}

#[tokio::test]
async fn smart_scan_filters_devices_by_the_allowlist_and_caps_at_64() {
    let inspect = healthy(&domovoi_core::identity::HostDirs::default());
    let exec = FakeProcessExecutor::new();
    let names = [
        "/dev/sdb",
        "/dev/sda",
        "/dev/nvme0n1",
        "/dev/nvme0",
        "/dev/sr0",
        "/dev/../x",
        "-h",
    ];
    let list: Vec<_> = names
        .iter()
        .map(|n| serde_json::json!({ "name": n }))
        .collect();
    exec.expect(
        SMARTCTL,
        &["--scan-open", "--json"],
        ok(serde_json::json!({ "devices": list }).to_string()),
    );
    let found = tools::smart::scan(env(&inspect, &exec), &patterns())
        .await
        .expect("scan");
    let texts: Vec<_> = found.iter().map(DeviceRef::as_str).collect();
    assert_eq!(texts, ["/dev/nvme0n1", "/dev/sda", "/dev/sdb"]);

    let many: Vec<_> = (0..200)
        .map(|i| serde_json::json!({ "name": format!("/dev/d{i}") }))
        .collect();
    exec.expect(
        SMARTCTL,
        &["--scan-open", "--json"],
        ok(serde_json::json!({ "devices": many }).to_string()),
    );
    let all = vec![
        "/dev/d[0-9]".to_owned(),
        "/dev/d[0-9][0-9]".to_owned(),
        "/dev/d[0-9][0-9][0-9]".to_owned(),
    ];
    assert_eq!(
        tools::smart::scan(env(&inspect, &exec), &all)
            .await
            .expect("scan")
            .len(),
        MAX_DEVICES
    );
    assert!(
        tools::smart::scan(env(&inspect, &exec), &[])
            .await
            .expect("scan")
            .is_empty(),
        "an empty list admits nothing"
    );
}

#[tokio::test]
async fn smart_scan_malformed_json_is_output_malformed() {
    let inspect = healthy(&domovoi_core::identity::HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(SMARTCTL, &["--scan-open", "--json"], ok("not json"));
    let err = tools::smart::scan(env(&inspect, &exec), &patterns())
        .await
        .expect_err("fails");
    assert_eq!(err.token, ReadToken::OutputMalformed);
    exec.expect(SMARTCTL, &["--scan-open", "--json"], ok("{\"other\":1}"));
    let err = tools::smart::scan(env(&inspect, &exec), &patterns())
        .await
        .expect_err("fails");
    assert_eq!(err.token, ReadToken::OutputMalformed);
}

const REPORT_ARGS: [&str; 4] = ["--all", "--json", "--nocheck=standby", "/dev/sda"];

#[tokio::test]
async fn smart_report_with_nonzero_exit_and_json_is_a_normal_report() {
    let inspect = healthy(&domovoi_core::identity::HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(
        SMARTCTL,
        &REPORT_ARGS,
        exited(8, "{\"smart_status\":{\"passed\":false}}", ""),
    );
    let report = tools::smart::report(env(&inspect, &exec), &patterns(), &dev("/dev/sda"))
        .await
        .expect("report");
    assert_eq!(report.exit_code, Some(8));
    assert!(report.json.contains("passed"));
    assert!(!report.truncated);
}

#[tokio::test]
async fn smart_report_with_nonzero_exit_and_no_output_is_command_failed() {
    let inspect = healthy(&domovoi_core::identity::HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(
        SMARTCTL,
        &REPORT_ARGS,
        exited(2, "", "Permission denied: /dev/sda\nmore"),
    );
    let err = tools::smart::report(env(&inspect, &exec), &patterns(), &dev("/dev/sda"))
        .await
        .expect_err("fails");
    assert_eq!(err.token, ReadToken::CommandFailed);
    assert_eq!(err.detail.as_deref(), Some("Permission denied: /dev/sda"));
}

#[tokio::test]
async fn smart_report_for_an_unlisted_device_spawns_nothing() {
    let dirs = domovoi_core::identity::HostDirs::default();
    let exec = std::sync::Arc::new(FakeProcessExecutor::new());
    let ctx = context(healthy(&dirs), "", clock(), exec.clone());
    let answer = ask(
        &ctx,
        HelperRequest::SmartReport {
            device: dev("/dev/sdaa"),
        },
    )
    .await;
    assert_eq!(
        answer,
        HelperResponse::ReadFailed {
            group: HelperGroup::SmartRead,
            token: ReadToken::DeviceNotAllowed,
            detail: None,
        }
    );
    assert!(exec.calls().is_empty());
}

#[test]
fn smart_pattern_matches_whole_strings_only() {
    assert!(smart_glob::matches("/dev/sd[a-z]", "/dev/sda"));
    assert!(!smart_glob::matches("/dev/sd[a-z]", "/dev/sdaa"));
    assert!(!smart_glob::matches("/dev/sd[a-z]", "x/dev/sda"));
    assert!(!smart_glob::matches("/dev/sd[a-z]", "/dev/sd"));
    assert!(!smart_glob::matches("/dev/sd[a-z]", "/dev/sd1"));
    assert!(smart_glob::matches("/dev/nvme[0-9]n1", "/dev/nvme0n1"));
    assert!(!smart_glob::matches("/dev/nvme[0-9]n1", "/dev/nvme0n10"));
    assert!(!smart_glob::matches("/dev/sd*", "/dev/sda"));
}

#[tokio::test]
async fn tool_timeout_is_timed_out() {
    let inspect = healthy(&domovoi_core::identity::HostDirs::default());
    let exec = FakeProcessExecutor::new();
    exec.expect(
        SMARTCTL,
        &["--scan-open", "--json"],
        ProcessOutput {
            timed_out: true,
            ..ProcessOutput::default()
        },
    );
    let err = tools::smart::scan(env(&inspect, &exec), &patterns())
        .await
        .expect_err("fails");
    assert_eq!(err.token, ReadToken::TimedOut);
    exec.expect_result(
        SMARTCTL,
        &["--scan-open", "--json"],
        Err(ProcessError::NotFound),
    );
    let err = tools::smart::scan(env(&inspect, &exec), &patterns())
        .await
        .expect_err("fails");
    assert_eq!(err.token, ReadToken::ToolMissing);
}

#[tokio::test]
async fn helper_spawns_only_the_three_read_tools_through_the_executor() {
    let dirs = domovoi_core::identity::HostDirs::default();
    let exec = std::sync::Arc::new(FakeProcessExecutor::new());
    let ctx = context(healthy(&dirs), "", clock(), exec.clone());
    for request in [
        HelperRequest::SmartScan,
        HelperRequest::SmartReport {
            device: dev("/dev/sda"),
        },
        HelperRequest::KernelLog {
            since_minutes: 5,
            lines: 10,
        },
        HelperRequest::NetListeners,
        HelperRequest::DockerList,
        HelperRequest::Health,
    ] {
        let _ = ask(&ctx, request).await;
    }
    let programs: std::collections::BTreeSet<_> =
        exec.calls().into_iter().map(|spec| spec.program).collect();
    let allowed: std::collections::BTreeSet<_> =
        ["/usr/sbin/smartctl", "/usr/bin/dmesg", "/usr/bin/ss"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    assert_eq!(programs, allowed);
    assert!(
        exec.calls()
            .iter()
            .all(|spec| spec.env.is_empty() && spec.run_as.is_none())
    );
}

#[tokio::test]
async fn smart_scan_through_the_socket_without_the_tool_is_tool_missing() {
    let dirs = domovoi_core::identity::HostDirs::default();
    let inspect =
        healthy(&dirs).lstat_result(SMARTCTL, Err(domovoi_core::ports::InspectError::NotFound));
    let exec = std::sync::Arc::new(FakeProcessExecutor::new());
    let ctx = context(inspect, "", clock(), exec.clone());
    assert_eq!(
        ask(&ctx, HelperRequest::SmartScan).await,
        HelperResponse::ReadFailed {
            group: HelperGroup::SmartRead,
            token: ReadToken::ToolMissing,
            detail: None,
        }
    );
    assert!(exec.calls().is_empty());
}
