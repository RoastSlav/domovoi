//! Invariant gates for the hub. Never delete or weaken these; extend them.
//!
//! Built into the `agent_server` test binary, which owns the scripted agent.

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
        Some(("x-hub-signature-256", format!("sha256={}", "0".repeat(64)))),
    )
    .await;
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(b"secret").unwrap();
    mac.update(b"a different body");
    let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    send(
        make_router(WebhookAuth::Hmac),
        Some(("x-hub-signature-256", signature)),
    )
    .await;
    send(
        make_router(WebhookAuth::Bearer),
        Some(("authorization", "Bearer wrong".to_owned())),
    )
    .await;
    assert!(sink.batches().is_empty());
}
