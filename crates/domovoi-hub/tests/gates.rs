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

/// Transports call services and never each other; a service never imports a transport or the
/// runtime; a new top-level module declares what it is; a transport never holds a command
/// handle (MOD-1 to MOD-4, ADR 0026). The same implementation `cargo xtask arch-check` runs.
#[test]
fn hub_modules_respect_the_module_edge_rule() {
    let violations = xtask::arch::modules::module_edge_violations(&hub_sources());
    assert!(
        violations.is_empty(),
        "the hub's module edges are broken: {violations:#?}",
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
const CAPS: &str = "fn f() {\n    self.link\n        .run_tool(a, b, c);\n    self.link\n        .start_skill(a, b, c, d, e);\n    self.link\n        .skill_result(a, b, c);\n    self.link\n        .run_check(a, b, c, d);\n    self.link\n        .send_transfer(a, b, c, d);\n    self.link\n        .start_capability(a, b, c, d, e);\n    self.link\n        .capability_result(a, b, c);\n    self.link\n        .start_unit_call(a, b, c, d, e);\n    self.link\n        .start_unit_call(a, b, c, d, e);\n    self.link\n        .unit_call_result(a, b, c);\n    self.link\n        .unit_call_result(a, b, c);
    self.link
        .attest_safety(a, b, c);\n}\n";

/// The smallest tree that satisfies the hub's boundary cards, including the two sites that
/// build a `Caller`.
fn hub_tree() -> Vec<(String, String)> {
    vec![
        (crate_path("runtime/caps.rs"), CAPS.to_owned()),
        (
            crate_path("api/mod.rs"),
            "fn f() { Caller::web(a, b); }
"
            .to_owned(),
        ),
        (
            crate_path("auth/interactions/mod.rs"),
            "fn f() { Caller::chat(a, b, c); }
"
            .to_owned(),
        ),
    ]
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
    let found = xtask::arch::ast::exec_violations(&tree);
    assert!(found.is_empty(), "a comment is not a call: {found:?}");
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
    use domovoi_core::Risk;
    use domovoi_hub::auth::csrf::NO_TOKEN_PATHS;
    use serde_json::json;

    use crate::api_harness::{Api, ORIGIN, SECRETS, api};

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
            (Method::POST, "/api/v1/conversations".to_owned()),
            (
                Method::POST,
                "/api/v1/conversations/0196ac01-0000-7000-8000-000000000001/messages".to_owned(),
            ),
            (
                Method::POST,
                "/api/v1/conversations/0196ac01-0000-7000-8000-000000000001/turns/0196ac01-0000-7000-8000-000000000001/cancel".to_owned(),
            ),
            (
                Method::POST,
                "/api/v1/conversations/0196ac01-0000-7000-8000-000000000001/messages/0196ac01-0000-7000-8000-000000000001/retry".to_owned(),
            ),
            (
                Method::PUT,
                "/api/v1/conversations/0196ac01-0000-7000-8000-000000000001/operation".to_owned(),
            ),
            (
                Method::DELETE,
                "/api/v1/conversations/0196ac01-0000-7000-8000-000000000001".to_owned(),
            ),
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

/// INV-1: a step becomes authorized only through the operation service. `.authorize(` on an
/// operation handle appears in the hub's sources only in `services/operations/steps.rs`
/// (the web endpoint and the chat button both reach it through `OperationCommands`), a step
/// is denied through a handle only there, and a `HumanApproval` is built only under
/// `src/auth/`. Whitespace is ignored, so a call split across lines is still seen.
#[test]
fn step_authorization_has_exactly_one_production_call_site() {
    let squeeze = |text: &str| text.split_whitespace().collect::<String>();
    let allowed_authorize = [crate_path("services/operations/steps.rs")];
    let auth_prefix = crate_path("auth/");
    let mut authorize_sites = Vec::new();
    let mut deny_sites = Vec::new();
    for (rel, text) in hub_sources() {
        let text = squeeze(&text);
        if text.contains(".authorize(") {
            authorize_sites.push(rel.clone());
        }
        if text.contains(".deny_from_chat(") || text.contains(".deny(op,step,") {
            deny_sites.push(rel.clone());
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
        "only the operation service may authorize a step"
    );
    deny_sites.sort();
    assert_eq!(
        deny_sites, allowed_authorize,
        "only the operation service may deny a step"
    );
}

mod chat {
    use domovoi_chat::discord::ids::custom_id;
    use domovoi_core::ports::{ActionRepo, InteractionSink};
    use domovoi_core::{ActionStatus, CardButton, InteractionKind, InteractionReply, Risk};

    use domovoi_hub::auth::interactions::{STALE_CARD, WEB_ONLY};

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
        for (button, reason) in [
            (approve(&action, &token), STALE_CARD),
            (
                CardButton::OpenInWeb {
                    action: action.id(),
                },
                WEB_ONLY,
            ),
        ] {
            let reply = c.authorizer.deliver(press(&a, ALLOWED, button)).await;
            assert_eq!(
                reply,
                InteractionReply::Rejected {
                    reason: reason.to_owned()
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

/// The workspace root, two levels above this crate.
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.rs` file under `dir` (workspace-relative), as (path, text).
fn tree(dir: &str) -> Vec<(String, String)> {
    let root = workspace_root();
    let mut files = Vec::new();
    source_files(&root.join(dir), &root.join(dir), dir, &mut files);
    files
}

/// The production sources of every crate except the test-support crate, as (path, text).
fn workspace_production() -> Vec<(String, String)> {
    let crates = workspace_root().join("crates");
    let mut all = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "domovoi-testkit" {
            all.extend(tree(&format!("crates/{name}/src")));
        }
    }
    all
}

/// Text with every whitespace character removed, so a call split across lines is still seen.
fn squeezed(text: &str) -> String {
    text.split_whitespace().collect()
}

/// The policy sources name no permit or authorization type, `PolicyPermit::issue(` and
/// `Authorization::policy(` each appear once in production, in the permit issuer's one file, the
/// word `PermitIssuer` is named only where the issuer, its port and its wiring live, and the
/// permit is built only after the evaluator's `Allow` was checked.
///
/// This replaces the earlier gate, which asserted that the issuer's directory did not exist. The
/// replacement is stricter: it names the one file and counts the calls.
#[test]
fn no_code_path_issues_a_policy_permit() {
    assert!(
        workspace_root()
            .join("crates/domovoi-hub/src/policy/issuer.rs")
            .exists(),
        "the permit issuer's one file exists"
    );
    let forbidden = [
        "PolicyPermit",
        "Authorization",
        "HumanApproval",
        "PermitIssuer",
    ];
    let mut checked = 0;
    let mut scanned: Vec<(String, String)> = Vec::new();
    for dir in [
        "crates/domovoi-core/src/policy_store",
        "crates/domovoi-store/src/policy",
        "crates/domovoi-engine/src/policy_shadow",
        "crates/domovoi-engine/src/policy",
    ] {
        scanned.extend(tree(dir));
    }
    scanned.extend(
        tree("crates/domovoi-hub/src")
            .into_iter()
            .filter(|(path, _)| {
                let file = path.rsplit('/').next().unwrap_or_default();
                (path.contains("/services/") && file.starts_with("policy"))
                    || path.ends_with("/runtime/policy.rs")
                    || (path.contains("/api/") && file.contains("policy"))
            }),
    );
    for (path, text) in &scanned {
        checked += 1;
        for word in forbidden {
            assert!(!text.contains(word), "{path} names {word}");
        }
    }
    assert!(
        checked >= 20,
        "the scan found the policy sources ({checked})"
    );
    // Where the word `PermitIssuer` may appear: the issuer, its port, the engine's permit
    // handler and its dependency group, and the runtime wiring.
    let may_name_issuer = |path: &str| {
        path.contains("crates/domovoi-hub/src/policy/")
            || path.ends_with("crates/domovoi-core/src/ports/permit.rs")
            || path.ends_with("crates/domovoi-core/src/ports/mod.rs")
            || path.ends_with("crates/domovoi-engine/src/operations/effects/policy.rs")
            || path.ends_with("crates/domovoi-engine/src/operations/deps.rs")
            || path.ends_with("crates/domovoi-engine/src/operations/mod.rs")
            || path.contains("crates/domovoi-hub/src/runtime/")
    };
    let mut issued: Vec<(String, usize, usize)> = Vec::new();
    for (path, text) in workspace_production() {
        let live = text.split("#[cfg(test)]").next().unwrap_or_default();
        let code: String = live
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        let code = squeezed(&code);
        let policy_calls = code.matches("Authorization::policy(").count();
        let issue_calls = code.matches("PolicyPermit::issue(").count();
        if policy_calls + issue_calls > 0 {
            issued.push((path.to_string(), policy_calls, issue_calls));
        }
        if code.contains("PermitIssuer") {
            assert!(may_name_issuer(&path), "{path} names PermitIssuer");
        }
    }
    assert_eq!(
        issued.len(),
        1,
        "the constructors appear in exactly one production file: {issued:?}"
    );
    let (path, policy_calls, issue_calls) = &issued[0];
    assert!(
        path.ends_with("crates/domovoi-hub/src/policy/issuer.rs"),
        "{path} builds a permit"
    );
    assert_eq!((*policy_calls, *issue_calls), (1, 1), "once each");
    // The permit is built only after the `Allow` outcome was checked.
    let issuer =
        std::fs::read_to_string(workspace_root().join("crates/domovoi-hub/src/policy/issuer.rs"))
            .unwrap_or_default();
    let allowed_at = issuer.find("Outcome::Allow");
    let built_at = issuer.find("PolicyPermit::issue(");
    assert!(
        matches!((allowed_at, built_at), (Some(allow), Some(build)) if allow < build),
        "the permit is built after the evaluator's `Allow` is required"
    );
}

/// Draft and publication writes are built only by the hub's policy service, and recorded
/// decisions only by the shadow recorder; the store's applier matches on them and builds none.
#[test]
fn policy_writes_are_built_only_by_their_owners() {
    let owners: [(&[&str], &str); 2] = [
        (
            &[
                "PolicyWrite::SaveDraft",
                "PolicyWrite::DiscardDraft",
                "PolicyWrite::Publish",
            ],
            "crates/domovoi-hub/src/services/policy",
        ),
        (
            &["PolicyWrite::RecordShadow"],
            "crates/domovoi-engine/src/policy_shadow/",
        ),
    ];
    let applier = "crates/domovoi-store/src/policy/";
    for (path, text) in workspace_production() {
        for (names, owner) in owners {
            for name in names {
                if text.contains(name) {
                    assert!(
                        path.starts_with(owner) || path.starts_with(applier),
                        "{path} names {name}, which only {owner} builds"
                    );
                }
            }
        }
    }
}

/// Every policy command takes the caller first, and publishing asks for the password before
/// it reads anything: a text-order scan, so reordering the body fails here.
#[test]
fn policy_commands_take_a_caller_and_step_up() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services");
    let mut text = String::new();
    for file in ["policy.rs", "policy_publish.rs"] {
        text.push_str(&std::fs::read_to_string(dir.join(file)).unwrap());
    }
    let text = squeezed(&text);
    for name in ["save_draft", "discard_draft", "publish"] {
        let head = format!("pubasyncfn{name}(&self,caller:&Caller,");
        assert!(text.contains(&head), "{name} takes `caller: &Caller` first");
    }
    let body = text.split("pubasyncfnpublish(").nth(1).unwrap_or_default();
    let step = body
        .find("self.step_up(")
        .expect("publish asks for the password");
    let read = body.find("self.repo.").expect("publish reads the draft");
    assert!(step < read, "publish asks for the password before it reads");
}

/// Publishing and changing a locked prohibition are enforced step-up actions, not future ones.
#[test]
fn policy_publish_is_a_registered_step_up_action() {
    use domovoi_hub::settings::step_up::{STEP_UP_ACTIONS, STEP_UP_ENFORCED_ACTIONS};
    let enforced: Vec<&str> = STEP_UP_ENFORCED_ACTIONS.iter().map(|a| a.id).collect();
    assert!(enforced.contains(&"policy.publish"));
    assert!(enforced.contains(&"policy.lock_changed"));
    assert!(STEP_UP_ACTIONS.iter().all(|a| a.id != "policy.publish"));
}

/// Past operations are searched across hosts only by the owner's search endpoint: the one
/// hub source that builds the global scope is `src/api/experience.rs`, once. Every other
/// path (the model's tool, chat, the context sources) stays on one host.
#[test]
fn experience_global_scope_is_built_only_by_the_owner_search_endpoint() {
    let sites: Vec<(String, usize)> = hub_sources()
        .into_iter()
        .map(|(rel, text)| (rel, text.matches("ExperienceScope::Global").count()))
        .filter(|(_, count)| *count > 0)
        .collect();
    assert_eq!(
        sites,
        [(crate_path("api/experience.rs"), 1)],
        "the global scope has one production builder"
    );
}

/// A summary edit reaches the operation writer through `OperationCommands` only, so the
/// caller, the actor and the audit row are decided in one place.
#[test]
fn summary_edit_has_one_production_call_site() {
    let mut sites: Vec<String> = hub_sources()
        .into_iter()
        .filter(|(_, text)| {
            text.split_whitespace()
                .collect::<String>()
                .contains(".edit_summary_field(")
        })
        .map(|(rel, _)| rel)
        .collect();
    sites.sort();
    let mut expected = vec![
        crate_path("api/experience.rs"),
        crate_path("services/operations/summary.rs"),
    ];
    expected.sort();
    assert_eq!(
        sites, expected,
        "the handler calls the service and the service calls the handle, nowhere else"
    );
    let handler = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/experience.rs"),
    )
    .unwrap();
    assert!(
        handler
            .split_whitespace()
            .collect::<String>()
            .contains(".operations.edit_summary_field("),
        "the endpoint goes through the operation commands"
    );
}

/// INV-1, INV-9: the Operator's HTTP code can reach no executor, approval or plan. It parses a
/// request, reads the world for the model and calls the conversation use cases, nothing more.
#[test]
fn operator_has_no_executor() {
    const FORBIDDEN: &[&str] = &[
        "HumanApproval",
        "Authorization",
        "ActionRequest",
        "ActionHandle",
        "OperationHandle",
        "AuthorizedExecution",
        "AuthorizedStep",
        "execute_step",
        "PlanHash",
        "ProcessExecutor",
        "AgentLink",
        "HandoffRunner",
        "run_skill",
        "run_capability",
        "start_skill",
        "PolicyPermit",
        "PackageHandle",
        "PackageCommands",
        "PackageCall",
        "InstallService",
        "web_package_authorization",
    ];
    fn scan(dir: &std::path::Path, files: &mut usize, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, files, offenders);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                *files += 1;
                let text = std::fs::read_to_string(&path).unwrap();
                for (number, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    for name in FORBIDDEN {
                        if line.contains(name) {
                            offenders.push(format!("{}:{}: {name}", path.display(), number + 1));
                        }
                    }
                }
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api/operator");
    let mut files = 0;
    let mut offenders = Vec::new();
    scan(&root, &mut files, &mut offenders);
    assert!(files >= 5, "the scan found only {files} files");
    assert!(
        offenders.is_empty(),
        "executor names in the Operator API: {offenders:?}"
    );
}

/// The files allowed to name the display-preferences service and its settings key.
const PREFERENCE_OWNERS: [&str; 4] = [
    "crates/domovoi-hub/src/services/preferences",
    "crates/domovoi-hub/src/services/mod.rs",
    "crates/domovoi-hub/src/api/preferences.rs",
    "crates/domovoi-hub/src/api/mod.rs",
];

/// The files that name the preferences service, build their settings key, or reach the
/// service through `services().preferences` outside the owners.
fn preference_leaks(files: &[(String, String)]) -> Vec<String> {
    let mut leaks = Vec::new();
    for (path, text) in files {
        let owner = PREFERENCE_OWNERS
            .iter()
            .any(|prefix| path.starts_with(prefix));
        let squeezed = squeezed(text);
        for needle in ["PreferenceService", "preferences_key", "\"preferences."] {
            if !owner && text.contains(needle) {
                leaks.push(format!("{path} names {needle}"));
            }
        }
        let reaches = squeezed.contains("services().preferences")
            || squeezed.contains("services.preferences");
        if reaches && path != "crates/domovoi-hub/src/api/preferences.rs" {
            leaks.push(format!("{path} reaches the preferences service"));
        }
    }
    leaks
}

/// Display preferences are presentation only: the service, its settings key and the handle on
/// it appear nowhere that decides, approves, audits or runs anything.
#[test]
fn display_preferences_never_reach_authority() {
    let files = workspace_production();
    assert!(
        files
            .iter()
            .any(|(path, _)| path == "crates/domovoi-hub/src/services/preferences.rs"),
        "the scan sees the preferences service"
    );
    let leaks = preference_leaks(&files);
    assert!(leaks.is_empty(), "preferences leak into: {leaks:#?}");
}

/// The preferences scan flags a forbidden use in a sample and lets the owners through.
#[test]
fn the_preferences_scan_flags_a_forbidden_use() {
    let sample = |path: &str, text: &str| vec![(path.to_owned(), text.to_owned())];
    for text in [
        "fn f(p: PreferenceService) {}",
        "fn f() { let k = preferences_key(user); }",
        "fn f() { store.get(\"preferences.abc\"); }",
        "fn f(s: &S) { s.services()\n    .preferences.get(&c); }",
    ] {
        let leaks = preference_leaks(&sample("crates/domovoi-hub/src/api/rules.rs", text));
        assert!(!leaks.is_empty(), "missed: {text}");
    }
    let owner = sample(
        "crates/domovoi-hub/src/services/preferences.rs",
        "fn k() { format!(\"preferences.{u}\"); PreferenceService }",
    );
    assert!(preference_leaks(&owner).is_empty());
    let handler = sample(
        "crates/domovoi-hub/src/api/preferences.rs",
        "fn h(s: &S) { s.services().preferences.get(&c); }",
    );
    assert!(preference_leaks(&handler).is_empty());
}

/// The text of a source file before its first `#[cfg(test)]`, so a test module does not count.
fn production_text(text: &str) -> &str {
    text.split("#[cfg(test)]").next().unwrap_or(text)
}

/// A package approval is made in one place and used in one: `web_package_authorization` is
/// defined in `auth/approval.rs` and called only by the package service's create and retry,
/// and neither a transport, the chat authorizer nor the Operator names it or the install
/// service (ADR 0029).
#[test]
fn package_approval_has_one_production_site() {
    let definition = crate_path("auth/approval.rs");
    let service = crate_path("services/packages/jobs.rs");
    let mut sites = Vec::new();
    for (rel, text) in hub_sources() {
        let text = squeezed(production_text(&text));
        if text.contains("web_package_authorization(") {
            sites.push(rel.clone());
        }
        if rel != service && rel != crate_path("services/packages/mod.rs") {
            for call in [
                "handle.create_job(",
                "handle.retry_target(",
                "handle.cancel_target(",
                "handle.refresh_inventory(",
            ] {
                assert!(
                    !text.contains(call) || rel.starts_with(&crate_path("services/packages/")),
                    "{rel} reaches the install service ({call}); only the package service does"
                );
            }
        }
    }
    sites.sort();
    assert_eq!(
        sites,
        vec![definition, service.clone()],
        "a package approval is made by auth/approval.rs and requested only by the package service"
    );
    let jobs = hub_sources()
        .into_iter()
        .find(|(rel, _)| *rel == service)
        .map(|(_, text)| squeezed(production_text(&text)))
        .unwrap_or_default();
    assert_eq!(
        jobs.matches("web_package_authorization(").count(),
        2,
        "one approval for a new job and one for a retry"
    );
    assert!(
        jobs.find("step_up(")
            .zip(jobs.find("web_package_authorization("))
            .is_some_and(|(password, approval)| password < approval),
        "the password is checked before an approval is made"
    );
}

/// No job comes from chat, the model or a transport other than the package endpoints: the
/// chat authorizer, the Operator API and the conversation service never name the install
/// service, the package commands, a package call or the approval constructor.
#[test]
fn jobs_never_come_from_the_model_or_chat() {
    const WORDS: &[&str] = &[
        "PackageHandle",
        "PackageCommands",
        "InstallService",
        "PackageCall",
        "PackageDispatch",
        "JobRequest",
        "web_package_authorization",
        "create_job",
        "retry_target",
        "refresh_inventory",
    ];
    let guarded = [
        "auth/interactions/",
        "api/operator/",
        "services/conversations.rs",
        "services/operations/",
        "services/incidents.rs",
    ];
    let mut checked = 0;
    for (rel, text) in hub_sources() {
        if !guarded.iter().any(|part| rel.contains(part)) {
            continue;
        }
        checked += 1;
        for word in WORDS {
            assert!(
                !production_text(&text).contains(word),
                "{rel} reaches package installation ({word})"
            );
        }
    }
    assert!(checked >= 10, "the scan found only {checked} sources");
    // Chat has no surface for it either: a chat caller is refused by the service itself.
    let service = hub_sources()
        .into_iter()
        .find(|(rel, _)| *rel == crate_path("services/packages/mod.rs"))
        .map(|(_, text)| text)
        .unwrap_or_default();
    assert!(
        service.contains("Surface::Chat { .. } => Err(PackageCommandError::WebOnly)"),
        "the package service no longer refuses a chat caller"
    );
    // The transports hold no handle: only the services layer names the install service.
    for (rel, text) in hub_sources() {
        if rel.contains("/api/") || rel.contains("/ingest/") {
            assert!(
                !production_text(&text).contains("PackageHandle"),
                "{rel} holds the install service handle"
            );
        }
    }
}

/// The hub cannot add, replace or remove a key an agent trusts. Its half: the owner keys are
/// built once from `hub.toml` and have no way to change, no hub source names a key-changing
/// request, and the package call the relay carries has no key field.
#[test]
fn hub_cannot_add_trusted_key() {
    const WORDS: &[&str] = &[
        "add_key",
        "AddKey",
        "revoke_key",
        "RevokeKey",
        "trusted_keys",
        "set_owner_key",
        "insert_owner",
    ];
    for (rel, text) in hub_sources() {
        for word in WORDS {
            assert!(
                !production_text(&text).contains(word),
                "{rel} names a key-changing request ({word})"
            );
        }
    }
    let keys = hub_sources()
        .into_iter()
        .find(|(rel, _)| *rel == crate_path("packages/keys.rs"))
        .map(|(_, text)| production_text(&text).to_owned())
        .unwrap_or_default();
    assert!(!keys.is_empty(), "packages/keys.rs was not read");
    assert!(
        !keys.contains("&mut self"),
        "PackageKeys has a method that changes it after it is built"
    );
    assert!(
        !keys.contains("pub fn insert") && !keys.contains("pub fn remove"),
        "PackageKeys has a way to add or remove a key"
    );
    // The setting paths that hold keys are file-only: the web settings layer names them as
    // refused, and no service writes `owner_keys`.
    for (rel, text) in hub_sources() {
        if rel.contains("/services/") || rel.contains("/api/") {
            assert!(
                !production_text(&text).contains("packages.owner_keys")
                    && !production_text(&text).contains("packages.revoked"),
                "{rel} reaches the owner keys"
            );
        }
    }
}

/// The host ceiling is read through the hub and never written: its handler names no settings
/// store, agent link or `agent.toml`, and declares only a `get`.
#[test]
fn host_ceiling_is_read_only_through_the_hub() {
    let text = hub_sources()
        .into_iter()
        .find(|(rel, _)| *rel == crate_path("api/capability_ceiling.rs"))
        .map(|(_, text)| production_text(&text).to_owned())
        .unwrap_or_default();
    assert!(!text.is_empty(), "api/capability_ceiling.rs was not read");
    for word in [
        "SettingsRepo",
        ".set(",
        "AgentLink",
        "agent.toml",
        "post,",
        "put,",
        "delete,",
        "patch,",
    ] {
        assert!(
            !text.contains(word),
            "the ceiling handler names {word}: it must stay read only"
        );
    }
    assert!(text.contains("get, path = \"/api/v1/hosts/{name}/ceiling\""));
}

/// The audit inspection port is read only here and only the audit routes use it: no chat
/// path, service, auth code or engine code names it, and the audit handlers never append.
#[test]
fn audit_inspection_is_read_only_and_only_the_audit_routes_use_it() {
    let sources = hub_sources();
    let violations = audit_inspect_violations(&sources);
    assert!(violations.is_empty(), "{violations:#?}");
    let sample = vec![
        (
            "crates/domovoi-hub/src/api/audit.rs".to_owned(),
            "x.audit_inspect.rows()".to_owned(),
        ),
        (
            "crates/domovoi-hub/src/chat/ask.rs".to_owned(),
            "deps.audit_inspect.head()".to_owned(),
        ),
        (
            "crates/domovoi-hub/src/api/dto_audit.rs".to_owned(),
            "audit.append(&e)".to_owned(),
        ),
    ];
    assert_eq!(
        audit_inspect_violations(&sample).len(),
        2,
        "the scan sees forbidden uses"
    );
}

fn audit_inspect_violations(sources: &[(String, String)]) -> Vec<String> {
    const ALLOWED: [&str; 3] = [
        "crates/domovoi-hub/src/api/audit.rs",
        "crates/domovoi-hub/src/api/mod.rs",
        "crates/domovoi-hub/src/runtime/mod.rs",
    ];
    let mut out = Vec::new();
    for (path, text) in sources {
        if text.contains("audit_inspect") && !ALLOWED.contains(&path.as_str()) {
            out.push(format!("{path} reaches the audit inspection port"));
        }
        let handler = path.ends_with("api/audit.rs") || path.ends_with("api/dto_audit.rs");
        if handler && text.contains(".append(") {
            out.push(format!("{path} appends to the audit log"));
        }
    }
    for (path, text) in tree("crates/domovoi-engine/src") {
        if text.contains("AuditInspect") {
            out.push(format!("{path} names AuditInspect"));
        }
    }
    out
}

/// The hub's production text of the file at `rel` under `src/`, or an empty string.
fn hub_text(rel: &str) -> String {
    let wanted = crate_path(rel);
    hub_sources()
        .into_iter()
        .find(|(path, _)| *path == wanted)
        .map(|(_, text)| text)
        .unwrap_or_default()
}

/// ATTEST-1: a safety report is verified in one module, through one function that takes the
/// pinned key. No other hub file names the signature check or a verifying key.
#[test]
fn attest_1_verify_strict_is_called_in_one_hub_module() {
    let mut sites = Vec::new();
    for (path, text) in hub_sources() {
        if !path.contains("helper_trust") && !path.contains("api/helper") {
            continue;
        }
        if text.contains("verify_strict") || text.contains("VerifyingKey") {
            sites.push(path);
        }
    }
    assert_eq!(sites, [crate_path("helper_trust/verify.rs")]);
    let verify = hub_text("helper_trust/verify.rs");
    assert_eq!(verify.matches(".verify_strict(").count(), 1);
    assert!(
        squeezed(&verify).contains("fnsignature_holds(pinned:&[u8;32]"),
        "the signature check takes the pinned key"
    );
    // The verifier is the only code that builds a verified report.
    for (path, text) in hub_sources() {
        if path.contains("helper_trust") && !path.ends_with("verify.rs") {
            assert!(
                !text.contains("VerifiedReport {"),
                "{path} builds a verified report"
            );
        }
    }
}

/// ATTEST-2: the acceptance write is built in `accept.rs` only, from a verified report, which
/// only the complete-report path of the verifier produces.
#[test]
fn attest_2_accept_is_built_only_in_accept_rs() {
    let mut sites = Vec::new();
    for (path, text) in hub_sources() {
        let built = text.contains("HelperTrustWrite::Accept")
            || (path.contains("helper_trust") && text.contains("Accepted {"));
        if built {
            sites.push(path);
        }
    }
    assert_eq!(sites, [crate_path("helper_trust/accept.rs")]);
    let accept = hub_text("helper_trust/accept.rs");
    assert!(accept.contains("verified: Box<VerifiedReport>"));
}

/// ATTEST-3: the hub computes every helper fingerprint from key bytes. The wire types carry no
/// fingerprint, and nothing that reads a relayed report or identity names one.
#[test]
fn attest_3_the_hub_never_reads_an_agent_fingerprint() {
    for name in ["HelperIdentity", "SafetyAttestation"] {
        let text = std::fs::read_to_string(
            workspace_root().join("crates/domovoi-proto/src/messages/handshake.rs"),
        )
        .unwrap()
            + &std::fs::read_to_string(
                workspace_root().join("crates/domovoi-proto/src/messages/stream.rs"),
            )
            .unwrap();
        let at = text
            .find(&format!("pub struct {name}"))
            .expect("the wire type");
        let body = &text[at..at + text[at..].find("\n}").unwrap()];
        assert!(
            !body.contains("fingerprint"),
            "{name} carries a fingerprint"
        );
    }
    for rel in ["agent_server/link_helper.rs", "agent_server/live.rs"] {
        assert!(
            !hub_text(rel).contains("fingerprint"),
            "{rel} reads a fingerprint"
        );
    }
}

/// ATTEST-5: the high-water table is written only by the acceptance and pin writes of the
/// applier (and deleted for a removed host), and read only by the store's read module.
#[test]
fn attest_5_high_water_is_written_only_by_the_applier() {
    let mut writers = Vec::new();
    let mut readers = Vec::new();
    for (path, text) in tree("crates/domovoi-store/src") {
        let writes = text.contains("INSERT INTO helper_high_water")
            || text.contains("UPDATE helper_high_water")
            || text.contains("DELETE FROM helper_high_water")
            || text.contains("\"helper_high_water\"");
        if writes {
            writers.push(path.clone());
        }
        if text.contains("FROM helper_high_water") && !text.contains("DELETE FROM") {
            readers.push(path);
        }
    }
    assert_eq!(writers, ["crates/domovoi-store/src/helper_trust/apply.rs"]);
    assert_eq!(readers, ["crates/domovoi-store/src/helper_trust/read.rs"]);
}

/// ATTEST-7: trust follows the latest outcome. No per-host mismatch flag or set exists in the
/// helper trust code, the store or the migration.
#[test]
fn attest_7_there_is_no_sticky_per_host_flag() {
    let mut texts: Vec<(String, String)> = hub_sources()
        .into_iter()
        .filter(|(path, _)| path.contains("helper_trust"))
        .collect();
    texts.extend(tree("crates/domovoi-store/src/helper_trust"));
    texts.push((
        "migration".to_owned(),
        std::fs::read_to_string(
            workspace_root().join("crates/domovoi-store/migrations/0020_helper_trust.sql"),
        )
        .unwrap(),
    ));
    for (path, text) in &texts {
        for banned in [
            "sticky",
            "mismatch_flag",
            "mismatched_hosts",
            "key_changed_hosts",
            "blocked_hosts",
        ] {
            assert!(
                !text.contains(banned),
                "{path} has a per-host flag ({banned})"
            );
        }
    }
    let migration = &texts.last().unwrap().1;
    assert!(migration.contains("latest_outcome"));
}

/// Production code of `text` with comments and everything from the first `#[cfg(test)]` removed.
fn live_code(text: &str) -> String {
    text.split("#[cfg(test)]")
        .next()
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Only the helper runs `systemctl restart`: no other production file names both.
#[test]
fn no_code_path_calls_systemctl_restart_outside_the_helper() {
    for (path, text) in workspace_production() {
        if path.starts_with("crates/domovoi-helper/") {
            continue;
        }
        let code = live_code(&text);
        let names_restart = code.contains("\"restart\"");
        assert!(
            !(names_restart && code.contains("systemctl")),
            "{path} builds a systemctl restart outside the helper"
        );
    }
}

/// The helper's only `systemctl` argument vector is `restart -- <Id>`: its sources name no
/// other verb, never reload, never reset a failed unit, and never pass `--json` to `systemctl`.
#[test]
fn helper_never_calls_daemon_reload() {
    let mut scanned = 0;
    for (path, text) in tree("crates/domovoi-helper/src") {
        let code = live_code(&text);
        for verb in [
            "daemon-reload",
            "daemon-reexec",
            "reset-failed",
            "try-restart",
            "set-property",
            "\"stop\"",
            "\"kill\"",
        ] {
            assert!(!code.contains(verb), "{path} names `{verb}`");
        }
        scanned += 1;
    }
    assert!(scanned > 10, "the helper sources were found ({scanned})");
}

/// `--json` goes to `busctl` only: the file that builds the restart command never names it.
#[test]
fn no_code_path_passes_json_to_systemctl() {
    let restart = std::fs::read_to_string(
        workspace_root().join("crates/domovoi-helper/src/systemd/restart.rs"),
    )
    .unwrap_or_default();
    assert!(!restart.is_empty(), "the restart file was found");
    assert!(!live_code(&restart).contains("--json"));
    for (path, text) in workspace_production() {
        let code = live_code(&text);
        if code.contains("Tool::Systemctl") {
            assert!(
                !code.contains("--json"),
                "{path} passes --json near systemctl"
            );
        }
    }
}

/// `container.restart` has no dispatch path: it is on neither closed list, and no production
/// source names a Docker helper write operation.
#[test]
fn container_restart_has_no_dispatch_path() {
    use domovoi_core::capability::{EXECUTABLE_BUILTINS, POLICY_DISPATCHABLE};
    assert!(!EXECUTABLE_BUILTINS.contains(&"container.restart"));
    assert!(!POLICY_DISPATCHABLE.contains(&"container.restart"));
    for (path, text) in workspace_production() {
        if path.ends_with("tests.rs") || path.contains("/tests/") {
            continue;
        }
        let code = live_code(&text);
        for needle in [
            "DockerRestart",
            "docker_restart",
            "DockerStop",
            "docker_exec",
            "restart_container_api",
        ] {
            assert!(
                !code.contains(needle),
                "{path} names a Docker write operation `{needle}`"
            );
        }
    }
}

/// The list of contracts a policy may dispatch is closed and has one entry.
#[test]
fn policy_dispatchable_list_is_closed() {
    use domovoi_core::capability::POLICY_DISPATCHABLE;
    assert_eq!(POLICY_DISPATCHABLE, ["unit.restart"]);
}

/// A unit call carries a name, or an id with the hash it was approved against: no free field.
#[test]
fn unit_call_has_no_free_argument() {
    let source =
        std::fs::read_to_string(workspace_root().join("crates/domovoi-core/src/unit/call.rs"))
            .unwrap_or_default();
    let start = source.find("pub enum UnitCall").unwrap_or_default();
    let end = source[start..]
        .find("\n}\n")
        .map_or(source.len(), |at| start + at);
    let block = &source[start..end];
    assert!(block.contains("name: UnitName") && block.contains("id: UnitId"));
    assert!(block.contains("identity_hash: IdentityHash"));
    for free in ["String", "Vec<", "BTreeMap", "PathBuf", "&str"] {
        assert!(
            !block.contains(free),
            "UnitCall carries a free field `{free}`"
        );
    }
}

/// The engine never builds an authorization or a permit; it carries the one it is handed.
#[test]
fn engine_never_builds_an_authorization_for_permits() {
    for (path, text) in tree("crates/domovoi-engine/src") {
        let code = squeezed(&live_code(&text));
        for call in [
            "Authorization::policy(",
            "PolicyPermit::issue(",
            "PolicyPermit::restore(",
        ] {
            assert!(!code.contains(call), "{path} calls {call}");
        }
    }
}
