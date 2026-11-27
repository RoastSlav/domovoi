//! Signed webhook ingest through the real axum router.

#![cfg(test)]

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderValue, Request, StatusCode};
use domovoi_config::hub::{WebhookAuth, WebhookConfig, WebhookKind};
use domovoi_config::{Secret, SecretSource};
use domovoi_core::ports::{HostRepo, IngestError};
use domovoi_core::{HostKind, HostName};
use domovoi_hub::ingest::{IngestDeps, IngestState, router};
use domovoi_testkit::{FakeClock, InMemoryStore, RecordingSignalSink};
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha2::Sha256;
use tower::ServiceExt;

const BODY: &str = r#"{"host":"web-1","check":"backup.nightly","summary":"Backup failed"}"#;
const SECRET: &str = "test-secret";

struct Harness {
    router: axum::Router,
    clock: Arc<FakeClock>,
    hosts: Arc<InMemoryStore>,
    sink: Arc<RecordingSignalSink>,
}

impl Harness {
    fn new(auth: WebhookAuth) -> Self {
        Self::with_config(config(auth, None))
    }

    fn with_config(config: WebhookConfig) -> Self {
        let clock = Arc::new(FakeClock::new(
            "2026-10-31T12:00:00Z".parse::<Timestamp>().unwrap(),
        ));
        let hosts = Arc::new(InMemoryStore::new());
        let sink = Arc::new(RecordingSignalSink::new());
        let deps = IngestDeps {
            clock: clock.clone(),
            hosts: hosts.clone(),
            sink: sink.clone(),
        };
        let webhooks = BTreeMap::from([("alerts".to_owned(), config)]);
        let router = router(IngestState::new(webhooks, deps));
        Self {
            router,
            clock,
            hosts,
            sink,
        }
    }

    async fn send(&self, body: &str, auth: Option<(&str, String)>) -> axum::response::Response {
        let mut request = Request::post("/hooks/alerts")
            .body(Body::from(body.to_owned()))
            .unwrap();
        if let Some((name, value)) = auth {
            request.headers_mut().insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(&value).unwrap(),
            );
        }
        self.router.clone().oneshot(request).await.unwrap()
    }

    async fn hmac(&self, body: &str) -> axum::response::Response {
        self.send(body, Some(("x-domovoi-signature", signature(body))))
            .await
    }
}

fn config(auth: WebhookAuth, default_host: Option<&str>) -> WebhookConfig {
    WebhookConfig {
        kind: WebhookKind::Generic,
        auth,
        secret: Secret::resolved(
            SecretSource::Env("TEST_SECRET".to_owned()),
            SecretString::from(SECRET.to_owned()),
        ),
        default_host: default_host.map(|name| HostName::parse(name).unwrap()),
        host_label: None,
    }
}

fn signature(body: &str) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(SECRET.as_bytes()).unwrap();
    mac.update(body.as_bytes());
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn valid_hmac_is_accepted() {
    let harness = Harness::new(WebhookAuth::Hmac);
    let response = harness.hmac(BODY).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(harness.sink.batches().len(), 1);
}

#[tokio::test]
async fn invalid_or_missing_signature_is_401() {
    let harness = Harness::new(WebhookAuth::Hmac);
    for auth in [
        None,
        Some(("x-domovoi-signature", "sha256=00".to_owned())),
        Some(("x-domovoi-signature", signature("different"))),
    ] {
        let response = harness.send(BODY, auth).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert!(harness.sink.batches().is_empty());
}

#[tokio::test]
async fn bearer_token_mode() {
    let harness = Harness::new(WebhookAuth::Bearer);
    let good = harness
        .send(BODY, Some(("authorization", format!("Bearer {SECRET}"))))
        .await;
    assert_eq!(good.status(), StatusCode::ACCEPTED);
    let bad = harness
        .send(BODY, Some(("authorization", "Bearer wrong".to_owned())))
        .await;
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(harness.sink.batches().len(), 1);
}

#[tokio::test]
async fn unknown_webhook_is_404() {
    let harness = Harness::new(WebhookAuth::Hmac);
    let request = Request::post("/hooks/missing")
        .body(Body::from(BODY))
        .unwrap();
    let response = harness.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rate_limit_returns_429() {
    let harness = Harness::new(WebhookAuth::Hmac);
    for _ in 0..60 {
        assert_eq!(harness.hmac(BODY).await.status(), StatusCode::ACCEPTED);
    }
    assert_eq!(
        harness.hmac(BODY).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    harness.clock.advance(SignedDuration::from_secs(1));
    assert_eq!(harness.hmac(BODY).await.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn unauthenticated_traffic_does_not_spend_the_rate_limit() {
    let harness = Harness::new(WebhookAuth::Hmac);
    for _ in 0..70 {
        let response = harness.send(BODY, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    // A flood that cannot sign in must not make the real monitor's alert a 429.
    assert_eq!(harness.hmac(BODY).await.status(), StatusCode::ACCEPTED);
    assert_eq!(harness.sink.batches().len(), 1);
}

#[tokio::test]
async fn unknown_host_creates_external_host() {
    let harness = Harness::new(WebhookAuth::Hmac);
    assert_eq!(harness.hmac(BODY).await.status(), StatusCode::ACCEPTED);
    let host = harness
        .hosts
        .get_host_by_name(&HostName::parse("web-1").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(host.kind, HostKind::External);
    assert_eq!(harness.sink.batches()[0][0].host(), host.id);
}

#[tokio::test]
async fn unparsable_body_is_400() {
    let harness = Harness::new(WebhookAuth::Hmac);
    assert_eq!(
        harness.hmac("not json").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert!(harness.sink.batches().is_empty());
}

#[tokio::test]
async fn unresolvable_host_is_422() {
    let harness = Harness::new(WebhookAuth::Hmac);
    let body = r#"{"host":"Bad_Host","check":"a.b","summary":"x"}"#;
    assert_eq!(
        harness.hmac(body).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let no_host = r#"{"check":"a.b","summary":"x"}"#;
    assert_eq!(
        harness.hmac(no_host).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn default_host_and_sink_unavailable_are_handled() {
    let harness = Harness::with_config(config(WebhookAuth::Hmac, Some("fallback")));
    let body = r#"{"check":"a.b","summary":"x"}"#;
    assert_eq!(harness.hmac(body).await.status(), StatusCode::ACCEPTED);
    let unusable = r#"{"host":"Bad_Host","check":"a.b","summary":"x"}"#;
    assert_eq!(harness.hmac(unusable).await.status(), StatusCode::ACCEPTED);
    let fallback = harness
        .hosts
        .get_host_by_name(&HostName::parse("fallback").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(
        harness
            .sink
            .batches()
            .iter()
            .flatten()
            .all(|s| s.host() == fallback.id)
    );
    harness
        .sink
        .fail_next(IngestError::Unavailable("full".to_owned()));
    assert_eq!(
        harness.hmac(body).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn a_rejected_webhook_request_is_logged_with_hook_source_and_reason() {
    use domovoi_testkit::log_capture::LogCapture;

    let harness = Harness::new(WebhookAuth::Hmac);
    let (logs, _guard) = LogCapture::start();

    assert_eq!(
        harness.send(BODY, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let line = logs.lines_with("webhook request rejected").remove(0);
    for field in [
        "WARN",
        "hook=\"alerts\"",
        "source=",
        "status=401",
        "no x-domovoi-signature header",
        "suppressed=0",
    ] {
        assert!(line.contains(field), "lacks {field}: {line}");
    }
    assert!(!line.contains(SECRET), "the secret never appears: {line}");

    assert_eq!(
        harness.hmac("not json").await.status(),
        StatusCode::BAD_REQUEST
    );
    let line = logs.lines_with("status=400").remove(0);
    assert!(line.contains("not a valid generic payload"), "{line}");

    let big = "x".repeat(domovoi_checks::sources::MAX_BODY_BYTES + 1);
    assert_eq!(
        harness.hmac(&big).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let line = logs.lines_with("status=413").remove(0);
    assert!(line.contains("larger than"), "{line}");
}

#[tokio::test]
async fn repeated_webhook_rejections_are_counted_not_repeated() {
    use domovoi_testkit::log_capture::LogCapture;

    let harness = Harness::new(WebhookAuth::Hmac);
    let (logs, _guard) = LogCapture::start();
    for _ in 0..50 {
        harness.send(BODY, None).await;
    }
    assert_eq!(
        logs.lines_with("webhook request rejected").len(),
        1,
        "one line for fifty identical rejections"
    );
}

#[tokio::test]
async fn a_rate_limited_webhook_names_the_hook_in_the_log() {
    use domovoi_testkit::log_capture::LogCapture;

    let harness = Harness::new(WebhookAuth::Hmac);
    for _ in 0..60 {
        harness.hmac(BODY).await;
    }
    let (logs, _guard) = LogCapture::start();
    assert_eq!(
        harness.hmac(BODY).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let line = logs.lines_with("status=429").remove(0);
    assert!(
        line.contains("hook=\"alerts\"") && line.contains("rate limit"),
        "{line}"
    );
}

#[tokio::test]
async fn random_unknown_hook_names_share_one_log_line() {
    use domovoi_testkit::log_capture::LogCapture;

    let harness = Harness::new(WebhookAuth::Hmac);
    let (logs, _guard) = LogCapture::start();
    for n in 0..30 {
        let request = Request::post(format!("/hooks/random-{n}"))
            .body(Body::from(BODY))
            .unwrap();
        let response = harness.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(
        logs.lines_with("webhook request rejected").len(),
        1,
        "{}",
        logs.text()
    );
}
