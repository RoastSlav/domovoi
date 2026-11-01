//! Invariant gates for the hub. Never delete or weaken these; extend them.
//!
//! Built into the `hub` test binary, which owns the scripted agent and the API harness.

use domovoi_core::ReadToolCall;
use domovoi_core::ports::{AgentLink, LinkError, Presence};
use domovoi_proto::{Message, PROTOCOL_VERSION};
use jiff::SignedDuration;

use crate::agent::{FakeAgent, enroll, new_key};
use crate::harness::{Hub, t0};

async fn reachable(hub: &Hub, host: domovoi_core::HostId) -> bool {
    let link = hub.server.link();
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
            hub.server.link().presence(ident.host),
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
            let reply = a.post(&uri, &login, confirm.clone()).await;
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{confirm}");
            let stored = domovoi_core::ports::ActionRepo::get(&*a.store, action.id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stored.status(), &ActionStatus::Proposed);
        }
        let reply = a
            .post(&uri, &login, json!({ "confirm_text": "restart_container" }))
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
