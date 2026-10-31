//! The Docker source against a mock Engine API.

use domovoi_checks::containers::{ContainerSource, DockerSource, Health, RunState};
use jiff::Timestamp;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::harness::read_fixture;

async fn serve(server: &MockServer, pattern: &str, fixture: &str) {
    Mock::given(method("GET"))
        .and(path_regex(pattern))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(read_fixture(fixture), "application/json"),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn docker_source_maps_api_responses() {
    let server = MockServer::start().await;
    serve(
        &server,
        r"^(/v[0-9.]+)?/containers/json$",
        "docker/containers.json",
    )
    .await;
    serve(
        &server,
        r"^(/v[0-9.]+)?/containers/aaa111/json$",
        "docker/inspect_aaa111.json",
    )
    .await;
    serve(
        &server,
        r"^(/v[0-9.]+)?/containers/bbb222/json$",
        "docker/inspect_bbb222.json",
    )
    .await;
    serve(
        &server,
        r"^(/v[0-9.]+)?/images/.*img1/json$",
        "docker/image_img1.json",
    )
    .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^(/v[0-9.]+)?/events$"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                r#"{"Type":"container","Action":"die","Actor":{"ID":"aaa111","Attributes":{"name":"plex","image":"plexinc/pms:latest"}},"time":1793000000,"timeNano":1793000000000000000}"#,
                "\n"
            ),
            "application/json",
        ))
        .mount(&server)
        .await;

    let source = DockerSource::connect(&format!("tcp://{}", server.address())).unwrap();
    let containers = source.list().await.unwrap();
    assert_eq!(containers.len(), 2);

    let plex = &containers[0];
    assert_eq!(plex.name, "plex");
    assert_eq!(plex.image, "plexinc/pms:latest");
    assert!(plex.managed() && plex.learnable());
    assert_eq!(plex.state, RunState::Running);
    assert_eq!(plex.health, Some(Health::Unhealthy));
    assert_eq!(plex.restart_count, 2);
    assert_eq!(
        plex.image_created,
        Some("2026-09-01T00:00:00Z".parse::<Timestamp>().unwrap())
    );
    assert_eq!(
        plex.started_at,
        Some(
            "2026-10-30T10:00:00.123456789Z"
                .parse::<Timestamp>()
                .unwrap()
        )
    );

    let scratch = &containers[1];
    assert_eq!(scratch.name, "scratch");
    assert!(scratch.auto_remove && !scratch.managed());
    assert_eq!(scratch.state, RunState::Exited);
    assert_eq!(scratch.started_at, None);
    assert_eq!(scratch.image_created, None);
    assert_eq!(scratch.health, None);

    let since = Timestamp::from_second(1_792_999_000).unwrap();
    let until = Timestamp::from_second(1_793_001_000).unwrap();
    let events = source.events_since(since, until).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "plex");
    assert_eq!(events[0].action, "die");
    assert_eq!(events[0].at, Timestamp::from_second(1_793_000_000).unwrap());
}

#[test]
fn docker_endpoints_other_than_unix_and_tcp_are_rejected() {
    assert!(DockerSource::connect("ssh://nas").is_err());
    assert!(DockerSource::connect("/var/run/docker.sock").is_err());
}

#[tokio::test]
async fn docker_inspect_omits_environment_values_and_logs_are_tailed() {
    let server = MockServer::start().await;
    serve(
        &server,
        r"^(/v[0-9.]+)?/containers/plex/json$",
        "docker/inspect_env.json",
    )
    .await;
    let mut frames = Vec::new();
    for (stream, line) in [
        (
            1u8,
            "starting
",
        ),
        (
            2u8,
            "warning: slow disk
",
        ),
    ] {
        frames.extend_from_slice(&[stream, 0, 0, 0]);
        frames.extend_from_slice(&u32::try_from(line.len()).unwrap().to_be_bytes());
        frames.extend_from_slice(line.as_bytes());
    }
    Mock::given(method("GET"))
        .and(path_regex(r"^(/v[0-9.]+)?/containers/plex/logs$"))
        .and(wiremock::matchers::query_param("tail", "50"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(frames, "application/vnd.docker.multiplexed-stream"),
        )
        .mount(&server)
        .await;

    let source = DockerSource::connect(&format!("tcp://{}", server.address())).unwrap();
    let inspect = source.inspect("plex").await.unwrap();
    let text = inspect.to_string();
    assert!(!text.contains("claim-secret-123"), "{text}");
    assert_eq!(
        inspect["Config"]["Env"],
        serde_json::json!(["PLEX_CLAIM", "TZ", "EMPTY"])
    );

    let logs = source.logs("plex", 50).await.unwrap();
    assert_eq!(
        logs,
        "starting
warning: slow disk
"
    );
}
