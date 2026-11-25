use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::helper::read::{
    ContainerRef, ContainerWire, DETAIL_MAX_BYTES, DeviceRef, MAX_CONTAINERS, MAX_EVENTS,
    MAX_LOG_TEXT_BYTES, ReadToken, WireRunState,
};
use domovoi_core::helper::{
    HelperGroup, HelperRequest, HelperResponse, MAX_RESPONSE_BYTES, UnavailableReason,
};
use domovoi_core::identity::HostDirs;
use domovoi_core::isolation::{
    CheckId, PLATFORM_RECORD_FORMAT, PlatformId, PlatformRecord, PlatformTier,
};
use domovoi_core::ports::{
    DockerReadError, ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec,
};
use domovoi_helper::serve::ServeContext;
use domovoi_testkit::{
    DockerReadCall, DockerReadScript, FakeClock, FakeDockerReadApi, FakeHostInspect,
    FakeProcessExecutor, InspectCall,
};
use jiff::SignedDuration;

use crate::support::{ask, clock, context, exited, healthy, ok};

fn dirs() -> HostDirs {
    HostDirs::default()
}

fn record(tier: PlatformTier, blocking: &[CheckId], kernel: &str) -> String {
    serde_json::to_string(&PlatformRecord {
        format: PLATFORM_RECORD_FORMAT,
        recorded_at: "2026-05-01T00:00:00Z".parse().expect("time"),
        kernel_release: kernel.to_owned(),
        platform: PlatformId {
            os_id: "debian".to_owned(),
            version_id: "13".to_owned(),
            arch: "x86_64".to_owned(),
        },
        tier,
        blocking: blocking.to_vec(),
    })
    .expect("record")
}

fn with_record(text: &str) -> FakeHostInspect {
    let d = dirs();
    healthy(&d)
        .path(
            d.helper_dir.join("platform.json"),
            crate::support::facts(domovoi_core::identity::PathKind::File, 0o600, 0),
        )
        .text(d.helper_dir.join("platform.json"), text)
        .kernel_release_text("6.1.0")
}

fn failed(group: HelperGroup, token: ReadToken) -> HelperResponse {
    HelperResponse::ReadFailed {
        group,
        token,
        detail: None,
    }
}

fn docker_ctx(inspect: FakeHostInspect, api: &Arc<FakeDockerReadApi>) -> ServeContext {
    context(inspect, "", clock(), Arc::new(FakeProcessExecutor::new()))
        .with_docker_read(api.clone())
}

fn wire(name: &str) -> ContainerWire {
    ContainerWire {
        name: name.to_owned(),
        image: "img".to_owned(),
        image_created: None,
        state: WireRunState::Running,
        health: None,
        restart_count: 0,
        labels: BTreeMap::new(),
        auto_remove: false,
        started_at: None,
    }
}

fn container(name: &str) -> ContainerRef {
    ContainerRef::parse(name).expect("container")
}

#[tokio::test]
async fn a_disabled_group_is_group_disabled_for_every_operation_of_the_group() {
    let exec = Arc::new(FakeProcessExecutor::new());
    let ctx = context(
        healthy(&dirs()),
        "groups = [\"safety.attest\"]\n",
        clock(),
        exec.clone(),
    );
    let dev = DeviceRef::parse("/dev/sda").expect("device");
    let cases = [
        (HelperGroup::DockerRead, HelperRequest::DockerProbe),
        (HelperGroup::DockerRead, HelperRequest::DockerList),
        (
            HelperGroup::DockerRead,
            HelperRequest::DockerEvents {
                since_secs: 60,
                until_secs: 0,
            },
        ),
        (
            HelperGroup::DockerRead,
            HelperRequest::DockerInspect {
                container: container("web"),
            },
        ),
        (
            HelperGroup::DockerRead,
            HelperRequest::DockerLogs {
                container: container("web"),
                lines: 5,
            },
        ),
        (HelperGroup::SmartRead, HelperRequest::SmartScan),
        (
            HelperGroup::SmartRead,
            HelperRequest::SmartReport { device: dev },
        ),
        (
            HelperGroup::KernelRead,
            HelperRequest::KernelLog {
                since_minutes: 5,
                lines: 5,
            },
        ),
        (HelperGroup::NetRead, HelperRequest::NetListeners),
    ];
    for (group, request) in cases {
        assert_eq!(
            ask(&ctx, request).await,
            HelperResponse::Unavailable {
                group,
                reason: UnavailableReason::GroupDisabled
            }
        );
    }
    assert!(exec.calls().is_empty());
}

#[tokio::test]
async fn unit_identity_is_served_when_systemd_read_is_enabled() {
    let name = || domovoi_core::unit::UnitName::parse("example.service").expect("name");
    // Enabled: the request reaches the unit operation. With no manager answering it is refused
    // as unreadable, never as `not_implemented` and never as a guess.
    let busctl = crate::support::tool_facts();
    let on = context(
        healthy(&dirs()).path("/usr/bin/busctl", busctl),
        "",
        clock(),
        Arc::new(FakeProcessExecutor::new()),
    );
    let answer = ask(&on, HelperRequest::UnitIdentity { name: name() }).await;
    assert!(
        matches!(&answer, HelperResponse::UnitRefused { token, .. } if token == "read_failed"),
        "{answer:?}"
    );

    // Disabled: the group gate answers before any program runs.
    let exec = Arc::new(FakeProcessExecutor::new());
    let off = context(
        healthy(&dirs()),
        "groups = [\"docker.read\"]\n",
        clock(),
        exec.clone(),
    );
    assert_eq!(
        ask(&off, HelperRequest::UnitIdentity { name: name() }).await,
        HelperResponse::Unavailable {
            group: HelperGroup::SystemdRead,
            reason: UnavailableReason::GroupDisabled
        }
    );
    assert!(exec.calls().is_empty());
}

#[tokio::test]
async fn unit_restart_is_gated_before_any_program_runs() {
    let exec = Arc::new(FakeProcessExecutor::new());
    let request = || HelperRequest::UnitRestart {
        id: domovoi_core::unit::UnitId::parse("example.service").expect("id"),
        identity_hash: "ab".repeat(32).parse().expect("hash"),
    };
    // A fresh install has no write group.
    let fresh = context(healthy(&dirs()), "", clock(), exec.clone());
    assert_eq!(
        ask(&fresh, request()).await,
        HelperResponse::Unavailable {
            group: HelperGroup::SystemdWrite,
            reason: UnavailableReason::GroupDisabled
        }
    );
    // The group is named but there is no platform record for this kernel: blocked.
    let named = context(
        healthy(&dirs()),
        "groups = [\"systemd.write\"]\n",
        clock(),
        exec.clone(),
    );
    assert_eq!(
        ask(&named, request()).await,
        HelperResponse::Unavailable {
            group: HelperGroup::SystemdWrite,
            reason: UnavailableReason::PlatformBlocked
        }
    );
    assert!(exec.calls().is_empty());
}

fn net_ok() -> Arc<FakeProcessExecutor> {
    let exec = Arc::new(FakeProcessExecutor::new());
    exec.expect(
        "/usr/bin/ss",
        &[
            "--no-header",
            "--listening",
            "--numeric",
            "--tcp",
            "--udp",
            "--processes",
        ],
        ok("tcp LISTEN 0 1 0.0.0.0:80 0.0.0.0:*\n"),
    );
    exec
}

fn served(answer: &HelperResponse) -> bool {
    matches!(answer, HelperResponse::NetListeners { .. })
}

#[tokio::test]
async fn reads_are_allowed_on_an_untested_platform() {
    for blocking in [vec![], vec![CheckId::St05, CheckId::St06, CheckId::St13]] {
        let tier = if blocking.is_empty() {
            PlatformTier::Untested
        } else {
            PlatformTier::Blocked
        };
        let inspect = with_record(&record(tier, &blocking, "6.1.0"));
        let ctx = context(inspect, "", clock(), net_ok());
        assert!(served(&ask(&ctx, HelperRequest::NetListeners).await));
    }
}

#[tokio::test]
async fn reads_are_refused_when_the_record_blocks_on_a_helper_isolation_check() {
    for check in [CheckId::St10, CheckId::St11, CheckId::St12, CheckId::St15] {
        let inspect = with_record(&record(
            PlatformTier::Blocked,
            &[CheckId::St05, check],
            "6.1.0",
        ));
        let exec = net_ok();
        let ctx = context(inspect, "", clock(), exec.clone());
        assert_eq!(
            ask(&ctx, HelperRequest::NetListeners).await,
            HelperResponse::Unavailable {
                group: HelperGroup::NetRead,
                reason: UnavailableReason::PlatformBlocked
            },
            "{check}"
        );
        assert!(exec.calls().is_empty());
    }
}

#[tokio::test]
async fn a_missing_or_stale_record_does_not_block_reads() {
    let blocking = record(PlatformTier::Blocked, &[CheckId::St10], "5.15.0");
    // Recorded on another kernel: stale, so ignored.
    let stale = with_record(&blocking);
    let ctx = context(stale, "", clock(), net_ok());
    assert!(served(&ask(&ctx, HelperRequest::NetListeners).await));
    // No record at all.
    let ctx = context(healthy(&dirs()), "", clock(), net_ok());
    assert!(served(&ask(&ctx, HelperRequest::NetListeners).await));
}

struct Hang;

#[async_trait]
impl ProcessExecutor for Hang {
    async fn run(&self, _spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn request_deadline_on_the_fake_clock_ends_a_slow_read() {
    let clock = clock();
    let ctx = ServeContext::new(
        domovoi_config::parse_helper("").map(|(c, _)| c),
        Arc::new(healthy(&dirs())),
        clock.clone(),
        dirs(),
        "test",
    )
    .with_process_executor(Arc::new(Hang));
    // Move the clock a second at a time until the deadline, set once the read starts, passes.
    let mut asking = std::pin::pin!(ask(&ctx, HelperRequest::NetListeners));
    let answer = loop {
        tokio::select! {
            answer = &mut asking => break answer,
            () = tokio::task::yield_now() => clock.advance(SignedDuration::from_secs(1)),
        }
    };
    assert_eq!(answer, failed(HelperGroup::NetRead, ReadToken::TimedOut));
}

#[tokio::test]
async fn response_larger_than_the_frame_cap_is_truncated_not_sent() {
    let exec = Arc::new(FakeProcessExecutor::new());
    // Control characters escape to six bytes each, so 1 MiB of them is over the cap as JSON.
    let json = "\u{1}".repeat(1024 * 1024);
    exec.expect(
        "/usr/sbin/smartctl",
        &["--all", "--json", "--nocheck=standby", "/dev/sda"],
        ok(json),
    );
    let ctx = context(healthy(&dirs()), "", clock(), exec);
    let answer = ask(
        &ctx,
        HelperRequest::SmartReport {
            device: DeviceRef::parse("/dev/sda").expect("device"),
        },
    )
    .await;
    let HelperResponse::SmartReport {
        truncated, json, ..
    } = &answer
    else {
        panic!("unexpected {answer:?}");
    };
    assert!(truncated);
    assert!(!json.is_empty());
    let frame = domovoi_core::helper::HelperResponseFrame {
        v: domovoi_core::helper::HELPER_PROTOCOL,
        id: uuid::Uuid::nil(),
        response: answer.clone(),
    };
    let bytes = domovoi_core::helper::encode_frame(&frame).expect("encodes");
    assert!(bytes.len() - 4 <= MAX_RESPONSE_BYTES);
}

#[tokio::test]
async fn read_failed_detail_is_bounded_ascii_and_has_no_paths() {
    let exec = Arc::new(FakeProcessExecutor::new());
    let stderr = format!(
        "ss: cannot open /proc/1/fd/3 and /etc/shadow caf\u{e9} {}\nsecond line",
        "x".repeat(400)
    );
    exec.expect(
        "/usr/bin/ss",
        &[
            "--no-header",
            "--listening",
            "--numeric",
            "--tcp",
            "--udp",
            "--processes",
        ],
        exited(1, "", &stderr),
    );
    let ctx = context(healthy(&dirs()), "", clock(), exec);
    let HelperResponse::ReadFailed { token, detail, .. } =
        ask(&ctx, HelperRequest::NetListeners).await
    else {
        panic!("expected a failure");
    };
    assert_eq!(token, ReadToken::CommandFailed);
    let detail = detail.expect("detail");
    assert!(detail.len() <= DETAIL_MAX_BYTES);
    assert!(detail.bytes().all(|b| b.is_ascii_graphic() || b == b' '));
    assert!(!detail.contains("/proc") && !detail.contains("/etc"));
    assert!(!detail.contains("second"));
}

#[tokio::test]
async fn a_read_never_writes_to_the_state_directory() {
    let inspect = Arc::new(with_record(&record(PlatformTier::Untested, &[], "6.1.0")));
    let ctx = ServeContext::new(
        domovoi_config::parse_helper("").map(|(c, _)| c),
        inspect.clone(),
        clock(),
        dirs(),
        "test",
    )
    .with_process_executor(net_ok());
    assert!(served(&ask(&ctx, HelperRequest::NetListeners).await));
    // The port has no mutating method; the only helper-state access is reading the record.
    let state = dirs().helper_dir;
    assert!(inspect.calls().iter().all(|call| match call {
        InspectCall::StateWriteTimings(..) => false,
        InspectCall::Probe(path, _) => !path.starts_with(&state),
        _ => true,
    }));
}

#[tokio::test]
async fn docker_without_an_api_is_engine_absent_and_errors_map_to_tokens() {
    let ctx = context(
        healthy(&dirs()),
        "",
        clock(),
        Arc::new(FakeProcessExecutor::new()),
    );
    assert_eq!(
        ask(&ctx, HelperRequest::DockerProbe).await,
        failed(HelperGroup::DockerRead, ReadToken::EngineAbsent)
    );
    let api = Arc::new(FakeDockerReadApi::new(DockerReadScript::default()));
    let ctx = docker_ctx(healthy(&dirs()), &api);
    let pairs = [
        (DockerReadError::Absent, ReadToken::EngineAbsent),
        (DockerReadError::Denied, ReadToken::EngineDenied),
        (DockerReadError::Refused, ReadToken::EngineApiRefused),
        (DockerReadError::NotFound, ReadToken::TargetNotFound),
        (
            DockerReadError::Engine("boom /var/run/docker.sock".to_owned()),
            ReadToken::EngineError,
        ),
        (DockerReadError::TimedOut, ReadToken::TimedOut),
    ];
    for (err, token) in pairs {
        api.set(DockerReadScript {
            inspect: Err(err),
            ..DockerReadScript::default()
        });
        let answer = ask(
            &ctx,
            HelperRequest::DockerInspect {
                container: container("web"),
            },
        )
        .await;
        assert_eq!(answer, failed(HelperGroup::DockerRead, token));
    }
}

#[tokio::test]
async fn docker_list_events_and_logs_are_capped_and_marked_truncated() {
    let list: Vec<_> = (0..=MAX_CONTAINERS)
        .map(|i| wire(&format!("c{i:04}")))
        .collect();
    let events: Vec<_> = (0..=MAX_EVENTS)
        .map(|i| domovoi_core::helper::read::EventWire {
            at: "2026-05-01T11:00:00Z".parse().expect("time"),
            name: format!("c{i}"),
            action: "die".to_owned(),
            image: None,
        })
        .collect();
    let api = Arc::new(FakeDockerReadApi::new(DockerReadScript {
        list: Ok(list),
        events: Ok(events),
        logs: Ok("a".repeat(MAX_LOG_TEXT_BYTES)),
        ..DockerReadScript::default()
    }));
    let ctx = docker_ctx(healthy(&dirs()), &api);
    let HelperResponse::DockerContainers {
        containers,
        truncated,
    } = ask(&ctx, HelperRequest::DockerList).await
    else {
        panic!("list");
    };
    assert_eq!((containers.len(), truncated), (MAX_CONTAINERS, true));
    let HelperResponse::DockerEvents { events, truncated } = ask(
        &ctx,
        HelperRequest::DockerEvents {
            since_secs: 3600,
            until_secs: 60,
        },
    )
    .await
    else {
        panic!("events");
    };
    assert_eq!((events.len(), truncated), (MAX_EVENTS, true));
    let HelperResponse::DockerLogs { text, truncated } = ask(
        &ctx,
        HelperRequest::DockerLogs {
            container: container("web"),
            lines: 10,
        },
    )
    .await
    else {
        panic!("logs");
    };
    assert_eq!((text.len(), truncated), (MAX_LOG_TEXT_BYTES, true));
    // The window is computed from the injected clock: 12:00 minus 3600 s and 60 s.
    let calls = api.calls();
    assert!(calls.contains(&DockerReadCall::Events(
        "2026-05-01T11:00:00Z".parse().expect("time"),
        "2026-05-01T11:59:00Z".parse().expect("time"),
    )));
    api.set(DockerReadScript {
        list: Ok(vec![wire("a")]),
        logs: Ok("short".to_owned()),
        ..DockerReadScript::default()
    });
    let HelperResponse::DockerContainers {
        containers,
        truncated,
    } = ask(&ctx, HelperRequest::DockerList).await
    else {
        panic!("list");
    };
    assert_eq!((containers.len(), truncated), (1, false));
    let HelperResponse::DockerLogs { truncated, .. } = ask(
        &ctx,
        HelperRequest::DockerLogs {
            container: container("web"),
            lines: 10,
        },
    )
    .await
    else {
        panic!("logs");
    };
    assert!(!truncated);
}

/// Every event of the process with the thread that made it, as one text line each. A global
/// subscriber avoids the per-callsite interest cache that a thread-local one races with when
/// tests run in parallel; the test reads back only its own thread's events.
static EVENTS: Mutex<Vec<(std::thread::ThreadId, String)>> = Mutex::new(Vec::new());

struct Capture;

struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={value:?}", field.name());
    }
}

impl tracing::Subscriber for Capture {
    fn register_callsite(
        &self,
        _: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }

    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields(event.metadata().level().to_string());
        event.record(&mut fields);
        EVENTS
            .lock()
            .expect("lock")
            .push((std::thread::current().id(), fields.0));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn helper_logs_one_line_per_read_without_the_response_body() {
    // Another test may have installed it already; either way it is the capture.
    let _ = tracing::subscriber::set_global_default(Capture);
    let exec = Arc::new(FakeProcessExecutor::new());
    exec.expect(
        "/usr/bin/ss",
        &[
            "--no-header",
            "--listening",
            "--numeric",
            "--tcp",
            "--udp",
            "--processes",
        ],
        ok("tcp LISTEN 0 1 0.0.0.0:80 0.0.0.0:* users:((\"SECRETNAME\",pid=5,fd=3))\n"),
    );
    let ctx = context(
        healthy(&dirs()),
        "",
        Arc::new(FakeClock::new("2026-05-01T12:00:00Z".parse().expect("t"))),
        exec,
    );
    assert!(served(&ask(&ctx, HelperRequest::NetListeners).await));
    let me = std::thread::current().id();
    let logged: Vec<String> = EVENTS
        .lock()
        .expect("lock")
        .iter()
        .filter(|(thread, _)| *thread == me)
        .map(|(_, line)| line.clone())
        .collect();
    let read_lines: Vec<_> = logged
        .iter()
        .filter(|l| l.contains("op=\"net_listeners\"") || l.contains("op=net_listeners"))
        .collect();
    assert_eq!(read_lines.len(), 1, "{logged:?}");
    let line = read_lines[0];
    assert!(line.starts_with("INFO"));
    assert!(line.contains("net.read") && line.contains("ok") && line.contains("peer_uid"));
    assert!(!line.contains("SECRETNAME") && !line.contains("0.0.0.0"));
}
