//! Invariant gates for the hub. Never delete or weaken these; extend them.
//!
//! Built into the `hub` test binary, which owns the scripted agent and the API harness.

use domovoi_core::ReadToolCall;
use domovoi_core::ports::{AgentLink, LinkError, Presence};
use domovoi_proto::{Message, PROTOCOL_VERSION};
use jiff::SignedDuration;

use crate::agent::{FakeAgent, enroll, new_key};
use crate::harness::{Hub, t0};

/// A transport reads host state through `AgentAdmin`; it never names, holds or takes the
/// agent link itself (EXEC-4, ADR 0023). The check is the workspace's one syntax-aware
/// implementation, shared with `cargo xtask arch-check`, default-deny over the hub's whole
/// source tree, so a transport directory added later is covered without an edit here and a
/// renamed import, a macro body or a split line cannot slip past.
#[test]
fn transports_do_not_name_the_agent_link() {
    let violations = xtask::arch::ast::exec_violations(&hub_sources());
    assert!(
        violations.is_empty(),
        "a transport names or reaches the agent link: {violations:#?}",
    );
}

/// The workspace-relative path of a file under the hub's `src/`.
fn crate_path(rel: &str) -> String {
    format!("crates/{}/src/{rel}", env!("CARGO_PKG_NAME"))
}

/// The hub's production sources: every `.rs` file under `src/` that no test target or
/// `#[cfg(test)]` module reaches, as (workspace-relative path, text).
fn hub_sources() -> Vec<(String, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let prefix = format!("crates/{}/src", env!("CARGO_PKG_NAME"));
    let mut files = Vec::new();
    source_files(&src, &src, &prefix, &mut files);
    let test_only = xtask::arch::ast::cfg_test_closure(&files);
    files
        .into_iter()
        .filter(|(rel, _)| !test_only.contains(rel))
        .collect()
}

/// Collects every `.rs` file under `dir` with its workspace-relative path and text.
fn source_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            source_files(root, &path, prefix, out);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.push((format!("{prefix}/{rel}"), text));
        }
    }
}

/// The hub's one raw-link consumer, holding its exact count: the capability forward every raw
/// link operation exactly once.
const CAPS: &str = "fn f() {\n    self.link\n        .run_tool(a, b, c);\n    self.link\n        .start_skill(a, b, c, d, e);\n    self.link\n        .skill_result(a, b, c);\n    self.link\n        .run_check(a, b, c, d);\n}\n";

/// The smallest tree that satisfies the hub's boundary cards.
fn hub_tree() -> Vec<(String, String)> {
    vec![(crate_path("runtime/caps.rs"), CAPS.to_owned())]
}

/// Appends `text` to `rel`'s file, adding the file when the tree does not have it.
fn push_text(tree: &mut Vec<(String, String)>, rel: &str, text: &str) {
    let path = crate_path(rel);
    match tree.iter_mut().find(|(file, _)| *file == path) {
        Some((_, body)) => body.push_str(text),
        None => tree.push((path, text.to_owned())),
    }
}

/// The capability layer answers "would the hub offer this here", never "may this run": it
/// cannot approve, cannot build an approval and cannot construct a `Contain` class.
#[test]
fn the_capability_layer_cannot_approve_anything() {
    for (path, text) in hub_sources() {
        if !path.contains("/src/capability/") {
            continue;
        }
        // The test module at the end of a file may build fixtures; production code is
        // everything before it.
        let production = text.split("#[cfg(test)]").next().unwrap_or(&text);
        for forbidden in [
            "HumanApproval",
            "PolicyPermit",
            "Authorization",
            "expected_safety",
            "EffectClass::Contain",
            "Contain {",
        ] {
            assert!(
                !production.contains(forbidden),
                "{path} mentions {forbidden}: availability is a report, never an approval",
            );
        }
        for call in ["run_skill(", "run_tool(", "start_skill(", ".approve("] {
            assert!(
                !production.contains(call),
                "{path} calls {call}: the capability layer dispatches nothing",
            );
        }
    }
}

/// A new transport source file dropped into the real hub tree is refused when it reaches the
/// raw link through an associated path: a call, a function item, a renamed type, a renamed
/// accessor, or the test-support accessor. The fixture is added to the hub's actual
/// production sources, so the gate is shown to cover a file nobody listed.
#[test]
fn a_new_transport_file_cannot_take_the_link_by_path() {
    let real = hub_sources();
    for (what, text) in [
        (
            "UFCS call",
            "fn f(s: &S) { let raw = AgentServer::link(&s); }
",
        ),
        (
            "function item",
            "fn f(s: &S) { let get = AgentServer::link; let raw = get(&s); }
",
        ),
        (
            "renamed type",
            "use crate::agent_server::AgentServer as Server;
fn f(s: &S) { let raw = Server::link(&s); }
",
        ),
        (
            "renamed accessor",
            "use crate::agent_server::AgentServer::link as obtain;
fn f(s: &S) { let raw = obtain(&s); }
",
        ),
        (
            "test accessor",
            "fn f(s: &S) { let raw = AgentServer::link_for_tests(&s); }
",
        ),
    ] {
        let mut tree = real.clone();
        tree.push((crate_path("sse/new_transport.rs"), text.to_owned()));
        let violations = xtask::arch::ast::exec_violations(&tree);
        assert!(
            violations
                .iter()
                .any(|v| v.rule == "EXEC-4" && v.location.contains("sse/new_transport.rs")),
            "{what} was accepted in a new transport file: {violations:#?}",
        );
    }

    let mut tree = real;
    tree.push((
        crate_path("sse/new_transport.rs"),
        "fn f() { let a = Other::link(&x); } // AgentServer::link
"
        .to_owned(),
    ));
    assert!(
        xtask::arch::ast::exec_violations(&tree).is_empty(),
        "an unrelated `link` is not the accessor",
    );
}

/// The scan sees an inferred accessor, and spares the composition root that legitimately
/// builds the boundaries from the link and the server module that owns it.
#[test]
fn the_link_scan_sees_an_untyped_accessor() {
    for text in [
        "let link = server.link();\n",
        "let presence = self.link().presence(host);\n",
        "let link = hub.server.link_for_tests();\n",
        "let link: Arc<dyn AgentLink> = server.link();\n",
        "let link = server\n    .link\n    ();\n",
    ] {
        for rel in ["api/hosts.rs", "sse/events.rs"] {
            let mut tree = hub_tree();
            push_text(&mut tree, rel, text);
            let violations = xtask::arch::ast::exec_violations(&tree);
            assert!(
                violations
                    .iter()
                    .any(|violation| violation.rule == "EXEC-4"),
                "{rel}: {text} was accepted in a transport",
            );
        }
    }
    let mut tree = hub_tree();
    push_text(
        &mut tree,
        "auth/interactions/web.rs",
        "// let link = server.link();\n",
    );
    assert!(
        xtask::arch::ast::exec_violations(&tree).is_empty(),
        "a comment is not a call",
    );
    for rel in ["runtime/mod.rs", "agent_server/mod.rs"] {
        let mut tree = hub_tree();
        push_text(&mut tree, rel, "let link = server.link();\n");
        assert!(
            !xtask::arch::ast::exec_violations(&tree)
                .iter()
                .any(|violation| violation.rule == "EXEC-4"),
            "{rel} is the composition root or the link's owner",
        );
    }
}

async fn reachable(hub: &Hub, host: domovoi_core::HostId) -> bool {
    let link = hub.server.link_for_tests();
    let call = link
        .run_tool(
            host,
            ReadToolCall::DockerPs { all: false },
            t0() + SignedDuration::from_secs(30),
        )
        .await;
    assert!(
        matches!(link.presence(host), Presence::Connected { .. }) || call.is_err(),
        "presence and requests agree"
    );
    call != Err(LinkError::NotConnected)
}

/// INV-10: the link reaches an agent only after it signed a fresh challenge with its
/// enrolled key. Enrolling, saying hello and answering with the wrong key all leave the
/// host unreachable.
#[tokio::test]
async fn agent_link_requires_completed_handshake() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    assert!(
        !reachable(&hub, ident.host).await,
        "enrolling opens no session"
    );

    let mut halfway = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = halfway.hello(ident.agent, PROTOCOL_VERSION).await else {
        panic!("expected a challenge");
    };
    assert!(
        !reachable(&hub, ident.host).await,
        "hello alone opens no session"
    );

    let reply = halfway.answer(&challenge, ident.agent, &new_key()).await;
    assert!(matches!(reply, Message::Reject(_)));
    assert!(
        !reachable(&hub, ident.host).await,
        "a wrong key opens no session"
    );

    let mut agent = FakeAgent::connect(hub.addr).await;
    agent.authenticate(&ident).await;
    assert!(
        matches!(
            hub.server.link_for_tests().presence(ident.host),
            Presence::Connected { .. }
        ),
        "the full handshake does"
    );
}

/// INV-12: no unauthenticated webhook request reaches the signal sink.
#[tokio::test]
async fn webhook_without_auth_never_reaches_signal_sink() {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{HeaderValue, Request, StatusCode};
    use domovoi_config::hub::{WebhookAuth, WebhookConfig, WebhookKind};
    use domovoi_config::{Secret, SecretSource};
    use domovoi_hub::ingest::{IngestDeps, IngestState, router};
    use domovoi_testkit::{FakeClock, InMemoryStore, RecordingSignalSink};
    use hmac::{Hmac, KeyInit, Mac};
    use secrecy::SecretString;
    use sha2::Sha256;
    use tower::ServiceExt;

    let body = r#"{"host":"nas","check":"a.b","summary":"x"}"#;
    let secret = || {
        Secret::resolved(
            SecretSource::Env("TEST_SECRET".to_owned()),
            SecretString::from("secret".to_owned()),
        )
    };
    let clock = Arc::new(FakeClock::new(jiff::Timestamp::constant(1_700_000_000, 0)));
    let sink = Arc::new(RecordingSignalSink::new());
    let make_router = |auth| {
        let config = WebhookConfig {
            kind: WebhookKind::Generic,
            auth,
            secret: secret(),
            default_host: None,
            host_label: None,
        };
        router(IngestState::new(
            BTreeMap::from([("alerts".to_owned(), config)]),
            IngestDeps {
                clock: clock.clone(),
                hosts: Arc::new(InMemoryStore::new()),
                sink: sink.clone(),
            },
        ))
    };
    let send = |router: axum::Router, header: Option<(&'static str, String)>| async move {
        let mut request = Request::post("/hooks/alerts")
            .body(Body::from(body))
            .unwrap();
        if let Some((name, value)) = header {
            request.headers_mut().insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(&value).unwrap(),
            );
        }
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    };

    send(make_router(WebhookAuth::Hmac), None).await;
    send(
        make_router(WebhookAuth::Hmac),
        Some(("x-domovoi-signature", format!("sha256={}", "0".repeat(64)))),
    )
    .await;
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(b"secret").unwrap();
    mac.update(b"a different body");
    let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    send(
        make_router(WebhookAuth::Hmac),
        Some(("x-domovoi-signature", signature)),
    )
    .await;
    send(
        make_router(WebhookAuth::Bearer),
        Some(("authorization", "Bearer wrong".to_owned())),
    )
    .await;
    assert!(sink.batches().is_empty());
}

// The web surface (INV-1, INV-5, INV-6, INV-8): every unsafe method needs the session's
// CSRF token and the hub's own origin, destructive approvals need the exact skill name,
// and settings never show a secret.
mod api {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use domovoi_core::{ActionStatus, Risk};
    use domovoi_hub::auth::csrf::NO_TOKEN_PATHS;
    use serde_json::json;

    use crate::api_harness::{Api, Login, ORIGIN, SECRETS, api};

    /// Every unsafe method and concrete path of the API, from the OpenAPI document, with
    /// path parameters filled in from `a`.
    async fn mutating_routes(a: &Api) -> Vec<(Method, String)> {
        let action = a.propose(Risk::Write).await;
        let note = domovoi_core::Note {
            id: domovoi_core::NoteId::new(),
            host: None,
            title: "n".to_owned(),
            body_markdown: String::new(),
            tags: Vec::new(),
            updated_at: a.now(),
        };
        domovoi_core::ports::NoteRepo::upsert(&*a.store, &note)
            .await
            .unwrap();
        let spec = domovoi_hub::router::openapi();
        let mut routes = Vec::new();
        for (path, item) in &spec.paths.paths {
            let concrete = path.replace("{name}", "nas").replace(
                "{id}",
                &if path.starts_with("/api/v1/actions") {
                    action.id().to_string()
                } else if path.starts_with("/api/v1/notes") {
                    note.id.to_string()
                } else {
                    a.incident.id().to_string()
                },
            );
            let methods = [
                (item.post.is_some(), Method::POST),
                (item.put.is_some(), Method::PUT),
                (item.patch.is_some(), Method::PATCH),
                (item.delete.is_some(), Method::DELETE),
            ];
            for (present, method) in methods {
                if present {
                    routes.push((method, concrete.clone()));
                }
            }
        }
        assert!(routes.len() >= 15, "the document lists the mutating routes");
        routes
    }

    async fn send(
        a: &Api,
        method: Method,
        uri: &str,
        cookie: &str,
        csrf: Option<&str>,
        origin: Option<&str>,
    ) -> StatusCode {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(csrf) = csrf {
            req = req.header("x-csrf-token", csrf);
        }
        if let Some(origin) = origin {
            req = req.header(header::ORIGIN, origin);
        }
        a.raw(req.body(Body::from("{}")).unwrap()).await.status
    }

    #[tokio::test]
    async fn unsafe_methods_without_csrf_are_403() {
        let a = api().await;
        let login = a.admin().await;
        for (method, uri) in mutating_routes(&a).await {
            if NO_TOKEN_PATHS.contains(&uri.as_str()) {
                continue;
            }
            for token in [None, Some("0".repeat(64).as_str()), Some("not-a-token")] {
                let status = send(&a, method.clone(), &uri, &login.cookie, token, None).await;
                assert_eq!(
                    status,
                    StatusCode::FORBIDDEN,
                    "{method} {uri} with {token:?}"
                );
            }
        }
        assert_eq!(
            a.get("/api/v1/session", &login).await.status,
            StatusCode::OK,
            "the session still works"
        );
    }

    #[tokio::test]
    async fn cross_origin_mutation_is_403() {
        let a = api().await;
        let login = a.admin().await;
        for (method, uri) in mutating_routes(&a).await {
            for origin in ["https://evil.example", "http://domovoi.example.net", "null"] {
                let status = send(
                    &a,
                    method.clone(),
                    &uri,
                    &login.cookie,
                    Some(&login.csrf),
                    Some(origin),
                )
                .await;
                assert_eq!(
                    status,
                    StatusCode::FORBIDDEN,
                    "{method} {uri} from {origin}"
                );
            }
        }
        let same = send(
            &a,
            Method::POST,
            &format!("/api/v1/incidents/{}/ack", a.incident.id()),
            &login.cookie,
            Some(&login.csrf),
            Some(ORIGIN),
        )
        .await;
        assert_eq!(same, StatusCode::NO_CONTENT, "the hub's own origin is fine");
    }

    #[tokio::test]
    async fn destructive_approve_requires_exact_confirm_text() {
        let a = api().await;
        let login: Login = a.admin().await;
        let action = a.propose(Risk::Destructive).await;
        let uri = format!("/api/v1/actions/{}/approve", action.id());
        for confirm in [
            json!({}),
            json!({ "confirm_text": null }),
            json!({ "confirm_text": "" }),
            json!({ "confirm_text": "RESTART_CONTAINER" }),
            json!({ "confirm_text": "restart_container " }),
            json!({ "confirm_text": " restart_container" }),
            json!({ "confirm_text": "restart" }),
        ] {
            let mut body = confirm.clone();
            body["plan_hash"] = json!(action.plan_hash_for_approval().to_string());
            let reply = a.post(&uri, &login, body).await;
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{confirm}");
            let stored = domovoi_core::ports::ActionRepo::get(&*a.store, action.id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stored.status(), &ActionStatus::Proposed);
        }
        let reply = a
            .post(
                &uri,
                &login,
                json!({
                    "confirm_text": "restart_container",
                    "plan_hash": action.plan_hash_for_approval().to_string(),
                }),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        assert_eq!(reply.body["status"], "approved");
    }

    #[tokio::test]
    async fn settings_never_return_secret_values() {
        let a = api().await;
        let login = a.admin().await;
        let reply = a.get("/api/v1/settings", &login).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        for (name, value) in SECRETS {
            assert!(!reply.text.contains(value), "{name} leaked");
        }
        assert_eq!(reply.body["llm_primary"]["api_key"], "[set]");
        assert_eq!(reply.body["discord"]["bot_token"], "[set]");
        assert_eq!(reply.body["webhooks"][0]["secret"], "[set]");
        assert_eq!(reply.body["heartbeat"]["url"], "[set]");
        assert_eq!(reply.body["llm_secondary"]["api_key"], "[unset]");

        // The write path: a save with a secret, an invalid save carrying one, and a
        // connection test that uses one never echo it.
        const UI_SECRET: &str = "ui-secret-value-never-returned";
        let mut bodies = Vec::new();
        let saved = a
            .send(
                Method::PATCH,
                "/api/v1/settings",
                Some(&login),
                Some(json!({
                    "base_revision": 0,
                    "password": crate::api_harness::PASSWORD,
                    "changes": { "discord": { "enabled": false, "channel_id": 1, "bot_token": { "set": UI_SECRET } } }
                })),
            )
            .await;
        assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
        bodies.push(saved.text);
        let invalid = a
            .send(
                Method::PATCH,
                "/api/v1/settings",
                Some(&login),
                Some(json!({
                    "base_revision": 1,
                    "password": crate::api_harness::PASSWORD,
                    "changes": { "heartbeat": { "url": { "set": UI_SECRET } }, "backup": { "keep": 0 } }
                })),
            )
            .await;
        assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
        bodies.push(invalid.text);
        let inline = a
            .send(
                Method::PATCH,
                "/api/v1/settings",
                Some(&login),
                Some(
                    json!({ "base_revision": 1, "changes": { "heartbeat": { "url": UI_SECRET } } }),
                ),
            )
            .await;
        assert_eq!(
            inline.status,
            StatusCode::BAD_REQUEST,
            "inline secrets are refused"
        );
        bodies.push(inline.text);
        let tested = a
            .post(
                "/api/v1/settings/test/heartbeat",
                &login,
                json!({ "changes": { "heartbeat": { "url": { "set": format!("http://127.0.0.1:9/{UI_SECRET}") } } } }),
            )
            .await;
        bodies.push(tested.text);
        bodies.push(a.get("/api/v1/settings", &login).await.text);
        for body in bodies {
            assert!(!body.contains(UI_SECRET), "a secret leaked: {body}");
            for (name, value) in SECRETS {
                assert!(!body.contains(value), "{name} leaked: {body}");
            }
        }
    }

    /// INV-8: settings writes need a logged-in admin and the CSRF token.
    #[tokio::test]
    async fn settings_writes_require_admin_session_and_csrf() {
        let a = api().await;
        let login = a.admin().await;
        let writes = [
            (Method::PATCH, "/api/v1/settings".to_owned()),
            (Method::POST, "/api/v1/settings/test/discord".to_owned()),
            (Method::POST, "/api/v1/settings/test/llm".to_owned()),
            (Method::POST, "/api/v1/settings/models".to_owned()),
            (Method::POST, "/api/v1/settings/test/heartbeat".to_owned()),
            (Method::PUT, "/api/v1/hosts/nas/checks".to_owned()),
            (Method::PUT, "/api/v1/hosts/nas/docker".to_owned()),
            (
                Method::PUT,
                "/api/v1/hosts/nas/skills/zpool_status".to_owned(),
            ),
            (Method::POST, "/api/v1/hosts/nas/profile".to_owned()),
            (Method::POST, "/api/v1/chat".to_owned()),
            (Method::POST, "/api/v1/system/update/check".to_owned()),
            (Method::POST, "/api/v1/ack-rules".to_owned()),
            (
                Method::DELETE,
                "/api/v1/ack-rules/0196ac01-0000-7000-8000-000000000000".to_owned(),
            ),
            (
                Method::POST,
                format!("/api/v1/incidents/{}/dismiss", a.incident.id()),
            ),
            (
                Method::POST,
                format!("/api/v1/incidents/{}/followups/clear", a.incident.id()),
            ),
        ];
        for (method, uri) in writes {
            let anonymous = send(&a, method.clone(), &uri, "", None, None).await;
            assert_eq!(
                anonymous,
                StatusCode::UNAUTHORIZED,
                "{method} {uri} without a session"
            );
            let no_csrf = send(&a, method.clone(), &uri, &login.cookie, None, None).await;
            assert_eq!(
                no_csrf,
                StatusCode::FORBIDDEN,
                "{method} {uri} without the CSRF token"
            );
        }
        assert_eq!(
            domovoi_core::ports::SettingsRepo::get(&*a.store, "config")
                .await
                .unwrap(),
            None,
            "nothing was saved"
        );
    }

    /// A credential posted to a webhook is redacted before the incident is stored, so the
    /// API never returns it.
    #[tokio::test]
    async fn webhook_secrets_never_reach_the_incident_api() {
        use std::collections::BTreeMap;
        use std::sync::Arc;

        use domovoi_config::hub::{WebhookAuth, WebhookConfig, WebhookKind};
        use domovoi_config::{Secret, SecretSource};
        use domovoi_core::ports::IncidentRepo;
        use domovoi_core::{CheckKey, Fingerprint};
        use domovoi_hub::ingest::{IngestDeps, IngestState, router};
        use secrecy::SecretString;
        use tower::ServiceExt;

        const LEAKS: [&str; 2] = ["hunter2hunter2", "ghp_abcdefghijklmnopqrstuvwxyz0123456789"];
        let a = api().await;
        let login = a.admin().await;
        let hooks = router(IngestState::new(
            BTreeMap::from([(
                "alerts".to_owned(),
                WebhookConfig {
                    kind: WebhookKind::Generic,
                    auth: WebhookAuth::Bearer,
                    secret: Secret::resolved(
                        SecretSource::Env("TEST_SECRET".to_owned()),
                        SecretString::from("hook-token".to_owned()),
                    ),
                    default_host: None,
                    host_label: None,
                },
            )]),
            IngestDeps {
                clock: a.clock.clone(),
                hosts: a.store.clone(),
                sink: Arc::new(a.manager.clone()),
            },
        ));
        let body = json!({
            "host": "nas",
            "check": "backup.nightly",
            "severity": "critical",
            "summary": "login failed with password=hunter2hunter2",
            "evidence": {
                "auth": "token ghp_abcdefghijklmnopqrstuvwxyz0123456789",
                "attempts": 3
            }
        })
        .to_string();
        // Two firing signals confirm the incident.
        for _ in 0..2 {
            let request = Request::post("/hooks/alerts")
                .header(header::AUTHORIZATION, "Bearer hook-token")
                .body(Body::from(body.clone()))
                .unwrap();
            let response = hooks.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::ACCEPTED);
        }

        let fingerprint =
            Fingerprint::of(a.nas.id, &CheckKey::parse("backup.nightly").unwrap(), None);
        let incident = IncidentRepo::find_active_by_fingerprint(&*a.store, &fingerprint)
            .await
            .unwrap()
            .expect("the webhook opened an incident");
        for uri in [
            format!("/api/v1/incidents/{}", incident.id()),
            "/api/v1/incidents".to_owned(),
        ] {
            let reply = a.get(&uri, &login).await;
            assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
            for leak in LEAKS {
                assert!(!reply.text.contains(leak), "{uri} returned {leak}");
            }
        }
        let detail = a
            .get(&format!("/api/v1/incidents/{}", incident.id()), &login)
            .await;
        assert!(detail.text.contains("[REDACTED]"), "{}", detail.text);
    }
}

/// Chat: what a Discord user can and cannot do (INV-1, INV-6).
/// Principle 1 and INV-1: no model tool and no chat command can read or change settings.
#[test]
fn no_tool_or_chat_command_reaches_settings() {
    for tool in domovoi_core::ReadTool::ALL {
        let name = tool.name();
        assert!(
            !name.contains("setting") && !name.contains("config") && !name.contains("secret"),
            "model tool {name} looks like it reaches settings"
        );
    }
    for command in domovoi_chat::discord::commands::NAMES {
        assert!(
            !command.contains("setting") && !command.contains("config"),
            "chat command {command} looks like it reaches settings"
        );
    }
    let interactions =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/auth/interactions");
    for entry in std::fs::read_dir(&interactions).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for forbidden in ["SettingsService", "crate::settings", "settings::"] {
            assert!(
                !text.contains(forbidden),
                "{} reaches the settings service ({forbidden})",
                path.display()
            );
        }
    }
}

/// INV-1: a step becomes authorized only through the step-approval endpoint. `.authorize(`
/// on an operation handle appears in the hub's sources only in `api/operations.rs`, and a
/// `HumanApproval` is built only under `src/auth/`. Whitespace is ignored, so a call split
/// across lines is still seen.
#[test]
fn step_approval_is_the_only_route_to_authorized() {
    let squeeze = |text: &str| text.split_whitespace().collect::<String>();
    let allowed_authorize = [
        crate_path("api/operations.rs"),
        crate_path("auth/interactions/buttons.rs"),
    ];
    let auth_prefix = crate_path("auth/");
    let mut authorize_sites = Vec::new();
    for (rel, text) in hub_sources() {
        let text = squeeze(&text);
        if text.contains(".authorize(") {
            authorize_sites.push(rel.clone());
        }
        if text.contains("HumanApproval::new(") {
            assert!(
                rel.starts_with(&auth_prefix),
                "{rel} builds a HumanApproval outside src/auth/"
            );
        }
    }
    authorize_sites.sort();
    assert_eq!(
        authorize_sites, allowed_authorize,
        "only the step-approval endpoint and the chat button handler may authorize a step"
    );
}

mod chat {
    use domovoi_chat::discord::ids::custom_id;
    use domovoi_core::ports::{ActionRepo, InteractionSink};
    use domovoi_core::{ActionStatus, CardButton, InteractionKind, InteractionReply, Risk};

    use crate::api_harness::api_with_triage;
    use crate::interactions::{ALLOWED, approve, chat, interaction, press, proposed, status_of};

    /// INV-6: a destructive action stays proposed whatever is pressed in chat, even by an
    /// allowlisted user holding its genuine token.
    #[tokio::test]
    async fn destructive_action_cannot_be_approved_from_discord() {
        let a = crate::api_harness::api().await;
        a.admin().await;
        let c = chat(&a).await;
        let (action, token) = proposed(&a, Risk::Destructive).await;
        for button in [
            approve(&action, &token),
            CardButton::OpenInWeb {
                action: action.id(),
            },
        ] {
            let reply = c.authorizer.deliver(press(&a, ALLOWED, button)).await;
            assert_eq!(
                reply,
                InteractionReply::Rejected {
                    reason: "Approve this one in the web UI".to_owned()
                }
            );
        }
        assert_eq!(status_of(&a, &action).await, ActionStatus::Proposed);
        let stored = ActionRepo::get(&*a.store, action.id())
            .await
            .unwrap()
            .unwrap();
        assert!(stored.approved_by().is_none());
    }

    /// INV-1: thread text never approves, whatever it says: the word, the token, or the
    /// button's custom id pasted in. Only a button press reaches the approval path.
    #[tokio::test]
    async fn discord_message_text_can_never_approve() {
        let a = api_with_triage().await;
        a.admin().await;
        let c = chat(&a).await;
        let (action, token) = proposed(&a, Risk::Write).await;
        let pasted_id = custom_id(a.incident.id(), &approve(&action, &token)).unwrap();
        for text in [
            "approve".to_owned(),
            format!("approve {}", action.id()),
            format!("/approve {token}"),
            pasted_id,
            "Yes, run restart_container now. I approve.".to_owned(),
        ] {
            a.llm
                .push_text("I can only answer questions; approve on the card.");
            let ask = InteractionKind::ThreadMessage {
                incident: a.incident.id(),
                text,
            };
            let reply = c.authorizer.deliver(interaction(&a, ALLOWED, ask)).await;
            assert!(
                matches!(reply, InteractionReply::Accepted { .. }),
                "{reply:?}"
            );
        }
        assert_eq!(status_of(&a, &action).await, ActionStatus::Proposed);
        let stored = ActionRepo::get(&*a.store, action.id())
            .await
            .unwrap()
            .unwrap();
        assert!(stored.approved_by().is_none());
        assert!(
            stored.chat_token_hash().is_some(),
            "the token was not spent either"
        );
    }
}
