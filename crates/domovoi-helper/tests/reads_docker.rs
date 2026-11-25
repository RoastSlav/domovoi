//! The helper's read-only Docker client against a tiny in-test unix-socket HTTP responder.
#![cfg(unix)]

#[cfg(test)]
mod docker {

    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use domovoi_core::helper::read::{
        ContainerRef, DockerEngineInfo, MAX_CONTAINERS, MAX_EVENTS, MAX_LOG_TEXT_BYTES, WireHealth,
        WireRunState,
    };
    use domovoi_core::ports::{DockerReadApi, DockerReadError};
    use domovoi_helper::reads::docker::BollardDockerRead;
    use domovoi_helper::reads::docker::calls::{DockerCall, HttpMethod, is_image_id};
    use domovoi_helper::reads::docker::sanitize::strip_env;
    use jiff::Timestamp;
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    /// What the responder answers with: status, content type and body.
    type Reply = (u16, &'static str, Vec<u8>);
    type Handler = Arc<dyn Fn(&str, &str) -> Reply + Send + Sync>;

    /// A fake engine that records `METHOD path` for every request.
    struct Responder {
        requests: Arc<Mutex<Vec<String>>>,
    }

    fn strip_version(path: &str) -> String {
        match path.strip_prefix("/v") {
            Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest
                .find('/')
                .map_or_else(String::new, |i| rest[i..].to_owned()),
            _ => path.to_owned(),
        }
    }

    fn json_reply(value: &Value) -> Reply {
        (200, "application/json", value.to_string().into_bytes())
    }

    impl Responder {
        fn start(path: &Path, handler: Handler) -> Self {
            let listener = UnixListener::bind(path).expect("bind");
            let requests = Arc::new(Mutex::new(Vec::new()));
            let seen = requests.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let seen = seen.clone();
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 4096];
                        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            match stream.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let head = String::from_utf8_lossy(&buf).into_owned();
                        let mut parts = head.lines().next().unwrap_or_default().split(' ');
                        let method = parts.next().unwrap_or_default().to_owned();
                        let path = strip_version(parts.next().unwrap_or_default());
                        seen.lock().expect("lock").push(format!("{method} {path}"));
                        let (status, kind, body) = if method == "GET" {
                            handler(&method, &path)
                        } else {
                            (500, "text/plain", b"not a read".to_vec())
                        };
                        let head = format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(head.as_bytes()).await;
                        let _ = stream.write_all(&body).await;
                        let _ = stream.shutdown().await;
                    });
                }
            });
            Self { requests }
        }

        fn seen(&self) -> Vec<String> {
            self.requests.lock().expect("lock").clone()
        }
    }

    fn socket_in(dir: &Path) -> PathBuf {
        dir.join("docker.sock")
    }

    fn inspect_doc(name: &str, image: &str) -> Value {
        json!({
            "Id": name, "Name": format!("/{name}"), "Image": image,
            "State": {"Status": "running", "StartedAt": "2026-01-01T00:00:00Z",
                      "Health": {"Status": "healthy"}},
            "RestartCount": 2, "HostConfig": {"AutoRemove": true},
            "Config": {"Image": "nginx:1", "Labels": {"a": "b"},
                       "Env": ["SECRET=hunter2", "PATH=/bin"]}
        })
    }

    /// A handler serving containers `ids` (name == id), with 404 for anything else.
    fn engine(ids: Vec<String>) -> Handler {
        Arc::new(move |_, path| {
            let path = path.split('?').next().unwrap_or_default();
            if path == "/containers/json" {
                let list: Vec<Value> = ids.iter().map(|id| json!({"Id": id})).collect();
                return json_reply(&Value::Array(list));
            }
            if let Some(id) = path
                .strip_prefix("/containers/")
                .and_then(|p| p.strip_suffix("/json"))
            {
                return if ids.iter().any(|i| i == id) {
                    json_reply(&inspect_doc(id, "sha256:aa11"))
                } else {
                    (
                        404,
                        "application/json",
                        br#"{"message":"No such container"}"#.to_vec(),
                    )
                };
            }
            if path == "/images/sha256:aa11/json" {
                return json_reply(
                    &json!({"Id": "sha256:aa11", "Created": "2025-12-01T00:00:00Z"}),
                );
            }
            (404, "application/json", br#"{"message":"no"}"#.to_vec())
        })
    }

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("c{i:04}")).collect()
    }

    fn client(sock: &Path) -> BollardDockerRead {
        BollardDockerRead::new(Some(&format!("unix://{}", sock.display())))
    }

    fn web() -> ContainerRef {
        ContainerRef::parse("web").expect("ref")
    }

    fn frame(stream: u8, text: &str) -> Vec<u8> {
        let mut out = vec![stream, 0, 0, 0];
        out.extend_from_slice(&u32::try_from(text.len()).expect("len").to_be_bytes());
        out.extend_from_slice(text.as_bytes());
        out
    }

    #[tokio::test]
    async fn docker_probe_prefers_the_configured_endpoint_then_the_two_system_sockets() {
        let dir = tempfile::tempdir().expect("dir");
        let configured = dir.path().join("custom.sock");
        let docker = dir.path().join("docker.sock");
        std::fs::create_dir(dir.path().join("podman")).expect("mkdir");
        let podman = dir.path().join("podman/podman.sock");
        let _a = UnixListener::bind(&configured).expect("bind");
        let _b = UnixListener::bind(&docker).expect("bind");
        let _c = UnixListener::bind(&podman).expect("bind");
        let fallback = || vec![docker.display().to_string(), podman.display().to_string()];

        let api = BollardDockerRead::with_candidates(
            Some(&format!("unix://{}", configured.display())),
            fallback(),
        );
        let info = api.engine().await.expect("engine");
        assert_eq!(
            info,
            DockerEngineInfo {
                engine: "Docker".into(),
                endpoint: configured.display().to_string()
            }
        );

        let api = BollardDockerRead::with_candidates(None, fallback());
        assert_eq!(
            api.engine().await.expect("engine").endpoint,
            docker.display().to_string()
        );

        std::fs::remove_file(&docker).expect("rm");
        let info = api.engine().await.expect("engine");
        assert_eq!(info.engine, "Podman");
        assert_eq!(info.endpoint, podman.display().to_string());

        let defaults = BollardDockerRead::new(None);
        drop(defaults);
        assert_eq!(
            domovoi_helper::reads::docker::client::DEFAULT_SOCKETS,
            ["/var/run/docker.sock", "/run/podman/podman.sock"]
        );
    }

    #[tokio::test]
    async fn docker_probe_reports_absent_and_denied_as_tokens() {
        let dir = tempfile::tempdir().expect("dir");
        let missing = dir.path().join("none.sock");
        let api = BollardDockerRead::with_candidates(None, [missing.display().to_string()]);
        assert_eq!(api.engine().await, Err(DockerReadError::Absent));
        assert_eq!(api.list().await, Err(DockerReadError::Absent));

        let locked = dir.path().join("locked.sock");
        let _l = UnixListener::bind(&locked).expect("bind");
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
                .expect("chmod");
        }
        if tokio::net::UnixStream::connect(&locked).await.is_ok() {
            return; // running as root: nothing is denied
        }
        let api = BollardDockerRead::with_candidates(
            None,
            [missing.display().to_string(), locked.display().to_string()],
        );
        assert_eq!(api.engine().await, Err(DockerReadError::Denied));
    }

    #[tokio::test]
    async fn docker_list_maps_every_field_and_sorts_by_name() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let ids = vec!["zeta".to_owned(), "web".to_owned(), "alpha".to_owned()];
        let _r = Responder::start(&sock, engine(ids));
        let list = client(&sock).list().await.expect("list");
        let order: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(order, ["alpha", "web", "zeta"]);
        let c = &list[1];
        assert_eq!(c.image, "nginx:1");
        assert_eq!(
            c.image_created,
            "2025-12-01T00:00:00Z".parse::<Timestamp>().ok()
        );
        assert_eq!(c.state, WireRunState::Running);
        assert_eq!(c.health, Some(WireHealth::Healthy));
        assert_eq!(c.restart_count, 2);
        assert_eq!(c.labels, BTreeMap::from([("a".to_owned(), "b".to_owned())]));
        assert!(c.auto_remove);
        assert_eq!(
            c.started_at,
            "2026-01-01T00:00:00Z".parse::<Timestamp>().ok()
        );
    }

    #[tokio::test]
    async fn docker_list_is_capped_at_500_and_marks_truncated() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let _r = Responder::start(&sock, engine(names(MAX_CONTAINERS + 10)));
        let list = client(&sock).list().await.expect("list");
        // One more than the cap is returned so the caller can tell the list was cut.
        assert_eq!(list.len(), MAX_CONTAINERS + 1);
        assert_eq!(list[0].name, "c0000");
    }

    #[tokio::test]
    async fn docker_list_skips_a_container_that_vanished() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let present = vec!["a".to_owned(), "c".to_owned()];
        let inner = engine(present);
        let handler: Handler = Arc::new(move |m, p| {
            if p.starts_with("/containers/json") {
                return json_reply(&json!([{"Id": "a"}, {"Id": "b"}, {"Id": "c"}]));
            }
            inner(m, p)
        });
        let _r = Responder::start(&sock, handler);
        let list = client(&sock).list().await.expect("list");
        let order: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(order, ["a", "c"]);
    }

    fn event_line(i: usize) -> String {
        json!({"Type": "container", "Action": "die",
           "Actor": {"ID": "x", "Attributes": {"name": format!("n{i}"), "image": "img"}},
           "time": 1_700_000_000 + i64::try_from(i).expect("i")})
        .to_string()
    }

    #[tokio::test]
    async fn docker_events_filter_type_container_and_cap_at_1000() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let seen_query = Arc::new(Mutex::new(String::new()));
        let q = seen_query.clone();
        let handler: Handler = Arc::new(move |_, path| {
            *q.lock().expect("lock") = path.to_owned();
            let mut body = String::new();
            for i in 0..MAX_EVENTS + 20 {
                body.push_str(&event_line(i));
                body.push('\n');
            }
            // An event with no container name is skipped.
            body.push_str(
                r#"{"Type":"container","Action":"x","Actor":{"ID":"y","Attributes":{}},"time":5}"#,
            );
            body.push('\n');
            (200, "application/json", body.into_bytes())
        });
        let _r = Responder::start(&sock, handler);
        let since = Timestamp::from_second(1_700_000_000).expect("ts");
        let until = Timestamp::from_second(1_700_100_000).expect("ts");
        let events = client(&sock).events(since, until).await.expect("events");
        assert_eq!(events.len(), MAX_EVENTS + 1);
        assert_eq!(events[0].name, "n0");
        assert_eq!(events[0].action, "die");
        assert_eq!(events[0].image.as_deref(), Some("img"));
        let query = seen_query.lock().expect("lock").clone();
        assert!(query.starts_with("/events?"), "{query}");
        assert!(query.contains("since=1700000000.000000000"), "{query}");
        assert!(
            query.contains("filters=%7B%22type%22%3A%5B%22container%22%5D%7D"),
            "{query}"
        );
    }

    #[tokio::test]
    async fn inspect_env_values_never_reach_the_response() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let _r = Responder::start(&sock, engine(vec!["web".to_owned()]));
        let doc = client(&sock).inspect(&web()).await.expect("inspect");
        assert_eq!(doc["Config"]["Env"], json!(["SECRET", "PATH"]));
        assert!(!doc.to_string().contains("hunter2"));
        assert!(!doc.to_string().contains("/bin"));
    }

    #[test]
    fn strip_env_port_matches_the_checks_function_on_the_same_fixture() {
        // The helper may not depend on domovoi-checks, so the expected outputs below are what the
        // checks crate's `strip_env` produces for the same inputs: every `Config.Env` entry
        // becomes the text before its first `=`, a non-string entry becomes the empty string and a
        // document without `Config.Env` is left as it is.
        let mut full = json!({"Id": "1", "Config": {"Image": "i", "Env": [
        "A=1", "B=x=y", "C=", "D", "=v", 7, null]}});
        strip_env(&mut full);
        assert_eq!(
            full,
            json!({"Id": "1", "Config": {"Image": "i", "Env": ["A", "B", "C", "D", "", "", ""]}})
        );
        for untouched in [
            json!({"Id": "1"}),
            json!({"Config": {}}),
            json!({"Config": {"Env": "A=1"}}),
            json!({"Config": {"Env": null}}),
        ] {
            let mut copy = untouched.clone();
            strip_env(&mut copy);
            assert_eq!(copy, untouched);
        }
    }

    #[tokio::test]
    async fn docker_inspect_not_found_is_target_not_found() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let _r = Responder::start(&sock, engine(vec![]));
        assert_eq!(
            client(&sock).inspect(&web()).await,
            Err(DockerReadError::NotFound)
        );
        assert_eq!(
            client(&sock).logs(&web(), 5).await,
            Err(DockerReadError::NotFound)
        );
    }

    #[tokio::test]
    async fn logs_are_cut_at_256_kib_on_a_utf8_boundary() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let seen_path = Arc::new(Mutex::new(String::new()));
        let p = seen_path.clone();
        let handler: Handler = Arc::new(move |_, path| {
            *p.lock().expect("lock") = path.to_owned();
            let text = format!("a{}", "\u{e9}".repeat(200_000));
            (
                200,
                "application/vnd.docker.multiplexed-stream",
                frame(1, &text),
            )
        });
        let _r = Responder::start(&sock, handler);
        let text = client(&sock).logs(&web(), 5).await.expect("logs");
        assert_eq!(text.len(), MAX_LOG_TEXT_BYTES - 1);
        assert!(text.ends_with('\u{e9}'));
        let path = seen_path.lock().expect("lock").clone();
        assert!(path.contains("tail=5"), "{path}");
        assert!(
            path.contains("stdout=true") || path.contains("stdout=1"),
            "{path}"
        );
    }

    #[tokio::test]
    async fn engine_403_and_405_are_engine_api_refused_and_not_retried() {
        for status in [403u16, 405] {
            let dir = tempfile::tempdir().expect("dir");
            let sock = socket_in(dir.path());
            let handler: Handler =
                Arc::new(move |_, _| (status, "application/json", br#"{"message":"no"}"#.to_vec()));
            let r = Responder::start(&sock, handler);
            let api = client(&sock);
            let now = Timestamp::from_second(1_700_000_000).expect("ts");
            assert_eq!(api.list().await, Err(DockerReadError::Refused));
            assert_eq!(api.inspect(&web()).await, Err(DockerReadError::Refused));
            assert_eq!(api.logs(&web(), 5).await, Err(DockerReadError::Refused));
            assert_eq!(api.events(now, now).await, Err(DockerReadError::Refused));
            // Each read was sent exactly once, and never as another verb or path.
            let seen = r.seen();
            assert_eq!(seen.len(), 4, "{seen:?}");
            assert!(seen.iter().all(|s| s.starts_with("GET ")));
        }
    }

    #[tokio::test]
    async fn docker_adapter_sends_only_get_requests_to_the_five_paths() {
        let dir = tempfile::tempdir().expect("dir");
        let sock = socket_in(dir.path());
        let inner = engine(vec!["web".to_owned()]);
        let handler: Handler = Arc::new(move |m, p| {
            if p.starts_with("/events") {
                return (200, "application/json", event_line(1).into_bytes());
            }
            if p.contains("/logs") {
                return (
                    200,
                    "application/vnd.docker.multiplexed-stream",
                    frame(1, "hi\n"),
                );
            }
            inner(m, p)
        });
        let r = Responder::start(&sock, handler);
        let api = client(&sock);
        let now = Timestamp::from_second(1_700_000_000).expect("ts");
        api.list().await.expect("list");
        api.inspect(&web()).await.expect("inspect");
        api.events(now, now).await.expect("events");
        assert_eq!(api.logs(&web(), 3).await.expect("logs"), "hi\n");

        let allowed: Vec<String> = [
            DockerCall::ListContainers,
            DockerCall::InspectContainer(&web()),
            DockerCall::InspectImage("sha256:aa11"),
            DockerCall::Events {
                since: now,
                until: now,
            },
            DockerCall::Logs {
                container: &web(),
                tail: 3,
            },
        ]
        .iter()
        .map(|c| {
            assert_eq!(c.method(), HttpMethod::Get);
            format!(
                "{} {}",
                c.method().as_str(),
                c.path().split('?').next().unwrap_or_default()
            )
        })
        .collect();
        let seen = r.seen();
        assert!(!seen.is_empty());
        for request in &seen {
            let bare = request.split('?').next().unwrap_or_default();
            assert!(
                allowed.iter().any(|a| a == bare),
                "unexpected request {request}"
            );
        }
        for expected in &allowed {
            assert!(
                seen.iter()
                    .any(|s| s.split('?').next() == Some(expected.as_str())),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn docker_call_paths_and_image_id_grammar() {
        assert!(is_image_id("sha256:0aF9"));
        assert!(is_image_id("0123abcd"));
        for bad in [
            "",
            "sha256:",
            "../x",
            "a/b",
            "sha256:xyz",
            "a?b=c",
            "latest",
        ] {
            assert!(!is_image_id(bad), "{bad}");
            assert!(DockerCall::inspect_image(bad).is_none());
        }
        assert_eq!(
            DockerCall::ListContainers.path(),
            "/containers/json?all=true"
        );
        assert_eq!(
            DockerCall::Logs {
                container: &web(),
                tail: 7
            }
            .path(),
            "/containers/web/logs?stdout=1&stderr=1&tail=7"
        );
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Method names after `docker.` or `Docker::` in `text`.
    fn bollard_calls(text: &str) -> Vec<String> {
        // A call split over lines (`docker\n    .list_containers(`) is joined first.
        let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = joined
            .replace("docker .", "docker.")
            .replace("Docker ::", "Docker::");
        let text = text.as_str();
        let mut found = Vec::new();
        for marker in ["docker.", "Docker::"] {
            for (at, _) in text.match_indices(marker) {
                let before = text[..at].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') && marker == "docker." {
                    continue; // e.g. `my_docker.`, not the client
                }
                let name: String = text[at + marker.len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let after = &text[at + marker.len() + name.len()..];
                if !name.is_empty() && after.trim_start().starts_with('(') {
                    found.push(name);
                }
            }
        }
        found
    }

    #[test]
    fn docker_read_client_calls_only_the_five_read_methods() {
        const ALLOWED: [&str; 6] = [
            "list_containers",
            "inspect_container",
            "inspect_image",
            "events",
            "logs",
            "connect_with_unix",
        ];
        const FORBIDDEN: [&str; 33] = [
            "create_",
            "start_",
            "stop_",
            "restart_",
            "kill_",
            "pause",
            "unpause",
            "remove_",
            "rename_",
            "update_",
            "prune_",
            "exec",
            "attach",
            "commit",
            "build",
            "import",
            "push",
            "tag_",
            "upload",
            "download",
            "resize",
            "wait",
            "connect_network",
            "disconnect_network",
            "post",
            "put",
            "delete",
            "head",
            "patch",
            "start",
            "stop",
            "restart",
            "kill",
        ];
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        assert!(!files.is_empty());
        let mut used = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("read");
            for name in bollard_calls(&text) {
                assert!(
                    ALLOWED.contains(&name.as_str()),
                    "{} calls the bollard method `{name}`",
                    file.display()
                );
                assert!(FORBIDDEN.iter().all(|f| !name.contains(f)), "{name}");
                used.push(name);
            }
        }
        for method in [
            "list_containers",
            "inspect_container",
            "inspect_image",
            "events",
            "logs",
        ] {
            assert!(
                used.iter().any(|u| u == method),
                "the client never calls {method}"
            );
        }
        // The scanner itself catches a write call.
        assert_eq!(
            bollard_calls("self.docker.restart_container(id, None).await"),
            ["restart_container"]
        );
    }
}
