use std::sync::Arc;
use std::time::Duration;

use domovoi_chat::discord::DiscordWebhookNotifier;
use domovoi_chat::discord::webhook::MAX_TRIES;
use domovoi_core::ports::{ChatError, Notifier, OutboundMessage};
use domovoi_testkit::FakeClock;
use jiff::SignedDuration;
use secrecy::SecretString;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const HOOK: &str = "/api/webhooks/1/hook-secret";

fn tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn digest() -> OutboundMessage {
    let line = format!("- {}", "x".repeat(97));
    OutboundMessage {
        title: "Daily digest, 2026-11-01".to_owned(),
        markdown: vec![line; 30].join("\n"),
        severity: None,
    }
}

async fn notifier(server: &MockServer, clock: Arc<FakeClock>) -> DiscordWebhookNotifier {
    tls();
    let url = SecretString::from(format!("{}{HOOK}", server.uri()));
    DiscordWebhookNotifier::new("discord-hook", url, clock).unwrap()
}

fn bodies(requests: &[Request]) -> Vec<Value> {
    requests
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

async fn wait_for_requests(server: &MockServer, n: usize) {
    for _ in 0..200 {
        if server.received_requests().await.unwrap_or_default().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("expected {n} requests");
}

#[tokio::test]
async fn webhook_notifier_splits_digest_and_retries_on_429() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(HOOK))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({ "retry_after": 2.5, "global": false })),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(HOOK))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let clock = Arc::new(FakeClock::new("2026-11-01T08:00:00Z".parse().unwrap()));
    let hook = Arc::new(notifier(&server, clock.clone()).await);

    let sending = {
        let hook = hook.clone();
        tokio::spawn(async move { hook.send(&digest()).await })
    };
    wait_for_requests(&server, 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "waits for the clock, not the wall"
    );
    clock.advance(SignedDuration::from_millis(2500));
    sending.await.unwrap().unwrap();

    let sent = bodies(&server.received_requests().await.unwrap());
    assert_eq!(
        sent.len(),
        3,
        "the rate-limited first part again, then the second"
    );
    assert_eq!(sent[0], sent[1], "the retry repeats the same message");
    assert_eq!(sent[0]["embeds"][0]["title"], "Daily digest, 2026-11-01");
    assert!(
        sent[2].get("embeds").is_none(),
        "only the first part has the title"
    );
    let parts: Vec<&str> = [&sent[1], &sent[2]]
        .iter()
        .map(|b| b["content"].as_str().unwrap())
        .collect();
    assert!(parts.iter().all(|p| p.chars().count() <= 2000));
    assert_eq!(
        parts.join("\n"),
        digest().markdown,
        "split on lines, nothing lost"
    );
}

#[tokio::test]
async fn webhook_gives_up_on_endless_rate_limits_and_hides_the_url() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({ "retry_after": 0.0 })))
        .mount(&server)
        .await;
    let clock = Arc::new(FakeClock::new("2026-11-01T08:00:00Z".parse().unwrap()));
    let hook = notifier(&server, clock).await;
    let err = hook.send(&digest()).await.unwrap_err();
    assert!(matches!(err, ChatError::RateLimited { .. }), "{err:?}");
    assert_eq!(
        server.received_requests().await.unwrap().len() as u32,
        MAX_TRIES
    );

    let failing = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&failing)
        .await;
    let clock = Arc::new(FakeClock::new("2026-11-01T08:00:00Z".parse().unwrap()));
    let err = notifier(&failing, clock)
        .await
        .send(&digest())
        .await
        .unwrap_err();
    assert!(matches!(err, ChatError::Rejected(_)));
    assert!(!format!("{err} {err:?}").contains("hook-secret"));

    let unreachable =
        SecretString::from("http://127.0.0.1:1/api/webhooks/1/hook-secret".to_owned());
    let clock = Arc::new(FakeClock::new("2026-11-01T08:00:00Z".parse().unwrap()));
    let err = DiscordWebhookNotifier::new("x", unreachable, clock)
        .unwrap()
        .send(&digest())
        .await
        .unwrap_err();
    assert!(matches!(err, ChatError::Transport(_)));
    assert!(!format!("{err} {err:?}").contains("hook-secret"), "{err}");
}
