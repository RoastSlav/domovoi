//! The typed helper reads and the container source built on them, against a stub helper.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_agent::helper_client::{HelperClient, HelperContainerSource, ReadClientError};
use domovoi_checks::containers::{ContainerSource, Health, MAX_CONTAINERS, RunState};
use domovoi_core::helper::read::{
    ContainerRef, ContainerWire, DeviceRef, DockerEngineInfo, EventWire, KernelLogOutput,
    ListenerWire, ReadToken, SmartReportOutput, WireHealth, WireProto, WireRunState,
};
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse, MAX_RESPONSE_BYTES};
use domovoi_core::ports::Clock;
use jiff::{SignedDuration, Timestamp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

use crate::harness::clock;
use crate::helper_stub::{Answer, serve};

pub fn wire(name: &str) -> ContainerWire {
    ContainerWire {
        name: name.to_owned(),
        image: "nginx:1.27".to_owned(),
        image_created: Some("2026-01-02T03:04:05Z".parse().unwrap()),
        state: WireRunState::Restarting,
        health: Some(WireHealth::Unhealthy),
        restart_count: 7,
        labels: BTreeMap::from([("com.docker.compose.project".to_owned(), "web".to_owned())]),
        auto_remove: true,
        started_at: Some("2026-02-03T04:05:06Z".parse().unwrap()),
    }
}

/// An answer for every read request.
pub fn answers() -> Answer {
    Arc::new(|request| match request {
        HelperRequest::DockerProbe => HelperResponse::DockerEngine {
            engine: Some(DockerEngineInfo {
                engine: "Docker".to_owned(),
                endpoint: "/var/run/docker.sock".to_owned(),
            }),
        },
        HelperRequest::DockerList => HelperResponse::DockerContainers {
            containers: vec![wire("web")],
            truncated: false,
        },
        HelperRequest::DockerEvents { .. } => HelperResponse::DockerEvents {
            events: vec![EventWire {
                at: "2026-03-04T05:06:07Z".parse().unwrap(),
                name: "web".to_owned(),
                action: "die".to_owned(),
                image: Some("nginx".to_owned()),
            }],
            truncated: false,
        },
        HelperRequest::DockerInspect { .. } => HelperResponse::DockerInspect {
            inspect: serde_json::json!({"Id": "abc"}),
        },
        HelperRequest::DockerLogs { .. } => HelperResponse::DockerLogs {
            text: "line\n".to_owned(),
            truncated: true,
        },
        HelperRequest::SmartScan => HelperResponse::SmartScan {
            devices: vec![DeviceRef::parse("/dev/sda").unwrap()],
        },
        HelperRequest::SmartReport { .. } => HelperResponse::SmartReport {
            json: "{}".to_owned(),
            exit_code: Some(4),
            truncated: false,
        },
        HelperRequest::KernelLog { .. } => HelperResponse::KernelLog {
            text: "oom\n".to_owned(),
            truncated: false,
        },
        HelperRequest::NetListeners => HelperResponse::NetListeners {
            listeners: vec![listener()],
            truncated: false,
        },
        _ => HelperResponse::ReadFailed {
            group: HelperGroup::DockerRead,
            token: ReadToken::CommandFailed,
            detail: None,
        },
    })
}

pub fn listener() -> ListenerWire {
    ListenerWire {
        proto: WireProto::Tcp,
        addr: "0.0.0.0".to_owned(),
        port: 22,
        process: Some("sshd".to_owned()),
        pid: Some(812),
    }
}

#[tokio::test]
async fn typed_client_methods_build_exactly_one_request_each() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let client = HelperClient::new(&socket);
    let clock = clock();
    let web = ContainerRef::parse("web").unwrap();
    let sda = DeviceRef::parse("/dev/sda").unwrap();
    let now = clock.now();

    let engine = client.docker_probe(&*clock).await.unwrap().unwrap();
    assert_eq!(engine.endpoint, "/var/run/docker.sock");
    let (containers, truncated) = client.docker_list(&*clock).await.unwrap();
    assert_eq!((containers.len(), truncated), (1, false));
    let since = now - SignedDuration::from_secs(300);
    let until = now - SignedDuration::from_secs(60);
    let (events, _) = client.docker_events(&*clock, since, until).await.unwrap();
    assert_eq!(events[0].action, "die");
    let inspect = client.docker_inspect(&*clock, &web).await.unwrap();
    assert_eq!(inspect["Id"], "abc");
    let logs = client.docker_logs(&*clock, &web, 50).await.unwrap();
    assert_eq!(logs, ("line\n".to_owned(), true));
    assert_eq!(client.smart_scan(&*clock).await.unwrap(), vec![sda.clone()]);
    let report = client.smart_report(&*clock, &sda).await.unwrap();
    assert_eq!(
        report,
        SmartReportOutput {
            json: "{}".to_owned(),
            exit_code: Some(4),
            truncated: false
        }
    );
    let kernel = client.kernel_log(&*clock, 30, 100).await.unwrap();
    assert_eq!(
        kernel,
        KernelLogOutput {
            text: "oom\n".to_owned(),
            truncated: false
        }
    );
    assert_eq!(
        client.net_listeners(&*clock).await.unwrap(),
        vec![listener()]
    );

    assert_eq!(
        stub.requests(),
        vec![
            HelperRequest::DockerProbe,
            HelperRequest::DockerList,
            HelperRequest::DockerEvents {
                since_secs: 300,
                until_secs: 60
            },
            HelperRequest::DockerInspect {
                container: web.clone()
            },
            HelperRequest::DockerLogs {
                container: web,
                lines: 50
            },
            HelperRequest::SmartScan,
            HelperRequest::SmartReport { device: sda },
            HelperRequest::KernelLog {
                since_minutes: 30,
                lines: 100
            },
            HelperRequest::NetListeners,
        ]
    );
}

#[tokio::test]
async fn client_refuses_a_response_header_above_the_response_cap() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("big.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0_u8; 4];
        stream.read_exact(&mut request).await.unwrap();
        let header = u32::try_from(MAX_RESPONSE_BYTES + 1).unwrap().to_be_bytes();
        stream.write_all(&header).await.unwrap();
        // Nothing follows: a client that tried to read the body would wait here.
        std::future::pending::<()>().await;
    });
    let client = HelperClient::new(&socket);
    let err = client.docker_list(&*clock()).await.unwrap_err();
    assert_eq!(err, ReadClientError::Protocol);
    task.abort();
}

#[tokio::test]
async fn failure_answers_become_typed_errors() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let _stub = serve(
        &socket,
        Arc::new(|_| HelperResponse::ReadFailed {
            group: HelperGroup::SmartRead,
            token: ReadToken::ToolMissing,
            detail: Some("smartctl".to_owned()),
        }),
    );
    let err = HelperClient::new(&socket)
        .smart_scan(&*clock())
        .await
        .unwrap_err();
    assert_eq!(
        err,
        ReadClientError::Failed(ReadToken::ToolMissing, Some("smartctl".to_owned()))
    );
    let absent = HelperClient::new(dir.path().join("none.sock"));
    assert_eq!(
        absent.smart_scan(&*clock()).await.unwrap_err(),
        ReadClientError::Missing
    );
}

fn source(socket: &std::path::Path) -> (HelperContainerSource, Arc<domovoi_testkit::FakeClock>) {
    let clock = clock();
    (
        HelperContainerSource::new(HelperClient::new(socket), clock.clone()),
        clock,
    )
}

#[tokio::test]
async fn container_source_maps_wire_to_container_info_field_for_field() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let _stub = serve(&socket, answers());
    let (source, _) = source(&socket);
    let containers = source.list().await.unwrap();
    assert_eq!(containers.len(), 1);
    let info = &containers[0];
    let expected = wire("web");
    assert_eq!(info.name, expected.name);
    assert_eq!(info.image, expected.image);
    assert_eq!(info.image_created, expected.image_created);
    assert_eq!(info.state, RunState::Restarting);
    assert_eq!(info.health, Some(Health::Unhealthy));
    assert_eq!(info.restart_count, 7);
    assert_eq!(info.labels, expected.labels);
    assert!(info.auto_remove);
    assert_eq!(info.started_at, expected.started_at);
}

#[tokio::test]
async fn container_source_truncates_to_the_container_cap() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let _stub = serve(
        &socket,
        Arc::new(|_| HelperResponse::DockerContainers {
            containers: (0..MAX_CONTAINERS + 5)
                .map(|i| wire(&format!("c{i}")))
                .collect(),
            truncated: true,
        }),
    );
    let (source, _) = source(&socket);
    assert_eq!(source.list().await.unwrap().len(), MAX_CONTAINERS);
}

#[tokio::test]
async fn container_source_never_sends_an_invalid_container_name() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let (source, _) = source(&socket);
    for name in ["../etc", "web;rm", "-rf", "", "a b", "/web"] {
        let inspect = source.inspect(name).await.unwrap_err();
        assert_eq!(inspect.0, "invalid container name", "{name:?}");
        let logs = source.logs(name, 10).await.unwrap_err();
        assert_eq!(logs.0, "invalid container name", "{name:?}");
    }
    assert_eq!(stub.count(), 0);
}

#[tokio::test]
async fn container_source_clamps_log_lines_and_the_event_window() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = serve(&socket, answers());
    let (source, clock) = source(&socket);
    let web = ContainerRef::parse("web").unwrap();
    source.logs("web", 0).await.unwrap();
    source.logs("web", 9_999).await.unwrap();
    let now: Timestamp = clock.now();
    let events = source
        .events_since(now - SignedDuration::from_hours(48), now)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        stub.requests(),
        vec![
            HelperRequest::DockerLogs {
                container: web.clone(),
                lines: 1
            },
            HelperRequest::DockerLogs {
                container: web,
                lines: 500
            },
            HelperRequest::DockerEvents {
                since_secs: 86_400,
                until_secs: 0
            },
        ]
    );
    // An empty window asks nothing.
    assert!(source.events_since(now, now).await.unwrap().is_empty());
    assert_eq!(stub.count(), 3);
}
