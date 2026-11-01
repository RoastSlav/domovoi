mod harness;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::http::StatusCode;
use domovoi_core::ports::{ChatError, Clock, MessageRef, NotifyStateRepo};
use domovoi_core::{AdvisoryKind, ChatRefs, Incident, NotifyState};
use domovoi_testkit::{GatewayCall, RecordingGateway, ScriptedLlm};
use jiff::{SignedDuration, Timestamp};
use metrics::{
    Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
};
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use self::harness::{BlockingLlm, Harness, REPORT, guard, until};

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn firing(host: &str, check: &str) -> String {
    format!(
        r#"{{"host":"{host}","check":"{check}","severity":"warning","summary":"{check} failed"}}"#
    )
}

fn resolving(host: &str, check: &str) -> String {
    format!(
        r#"{{"host":"{host}","check":"{check}","status":"resolved","summary":"{check} recovered"}}"#
    )
}

fn post_count(gateway: &RecordingGateway) -> usize {
    gateway
        .calls()
        .iter()
        .filter(|call| matches!(call, GatewayCall::PostCard(_)))
        .count()
}

async fn wait_for_incidents(harness: &Harness, count: usize) -> Vec<Incident> {
    guard(async {
        loop {
            let incidents = harness.incidents().await;
            if incidents.len() >= count {
                return incidents;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
}

#[tokio::test]
async fn end_to_end_signal_to_card_to_triage_update() {
    let llm = Arc::new(ScriptedLlm::new());
    llm.push_text(REPORT);
    let mut harness = Harness::with_adapters(
        "[triage]\nenabled = true\nconcurrency = 1",
        at("2026-10-31T12:00:00Z"),
        llm,
        Vec::new(),
        4096,
    )
    .await;
    harness.start_dispatcher();

    assert_eq!(
        harness.webhook(&firing("web-1", "backup.nightly")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            let updated = harness.gateway.calls().iter().any(|call| {
                matches!(call, GatewayCall::UpdateCard(_, card) if card.headline == "Backup needs attention")
            });
            if updated {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let incidents = harness.incidents().await;
    assert_eq!(incidents.len(), 1);
    assert_eq!(harness.signals(incidents[0].id()).await, 1);
    assert_eq!(post_count(&harness.gateway), 1);
    harness.stop().await;
}

#[tokio::test]
async fn delivery_retries_then_records_failure_without_blocking_others() {
    let bad = Arc::new(RecordingGateway::new("bad"));
    let good = Arc::new(RecordingGateway::new("good"));
    for _ in 0..12 {
        bad.fail_next(ChatError::Transport("offline".to_owned()));
    }
    let mut harness = Harness::with_adapters(
        "",
        at("2026-10-31T12:00:00Z"),
        Arc::new(ScriptedLlm::new()),
        vec![bad.clone(), good.clone()],
        4096,
    )
    .await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    until(|| post_count(&good) == 1).await;
    assert_eq!(
        harness.webhook(&firing("web-2", "backup.failed")).await,
        StatusCode::ACCEPTED
    );
    until(|| post_count(&good) == 2).await;
    assert_eq!(harness.clock.now(), at("2026-10-31T12:00:00Z"));

    for _ in 0..31 {
        harness.clock.advance(SignedDuration::from_secs(1));
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }
    let advisories = guard(async {
        loop {
            let advisories = harness.advisories().await;
            if advisories
                .iter()
                .any(|advisory| advisory.kind == AdvisoryKind::DeliveryFailed)
            {
                return advisories;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let failures: Vec<_> = advisories
        .iter()
        .filter(|advisory| advisory.kind == AdvisoryKind::DeliveryFailed)
        .collect();
    assert_eq!(failures.len(), 1);
    assert!(failures[0].detail.contains("deliveries exhausted"));
    assert_eq!(post_count(&good), 2);
    harness.stop().await;
}

#[tokio::test]
async fn message_references_survive_restart() {
    let mut harness = Harness::new("", at("2026-10-31T12:00:00Z")).await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    until(|| post_count(&harness.gateway) == 1).await;
    let incident = harness.incidents().await[0].id();

    let mut harness = harness.restart().await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&resolving("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            let updated = harness.gateway.calls().iter().any(|call| {
                matches!(call, GatewayCall::UpdateCard(message, card) if message.id == "msg-1" && card.incident == incident && card.resolved)
            });
            if updated {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(post_count(&harness.gateway), 1);
    harness.stop().await;
}

#[tokio::test]
async fn digest_sent_once_per_day_at_local_time() {
    let mut harness = Harness::new(
        "[notify.digest]\nenabled = true\ntime = \"09:00\"",
        at("2026-10-31T08:59:00Z"),
    )
    .await;
    harness.start_schedulers();
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    assert!(harness.notifier.sent().is_empty());
    harness.clock.set(at("2026-10-31T09:00:00Z"));
    until(|| harness.notifier.sent().len() == 1).await;

    let mut harness = harness.restart().await;
    harness.start_schedulers();
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(harness.notifier.sent().len(), 1);
    harness.clock.set(at("2026-11-01T09:00:00Z"));
    until(|| harness.notifier.sent().len() == 2).await;
    harness.stop().await;
}

#[tokio::test]
async fn quiet_hours_release_happens_at_end_time() {
    let mut harness = Harness::new(
        "[notify]\nquiet_hours = { start = \"22:00\", end = \"07:00\" }\n[notify.digest]\nenabled = false",
        at("2026-10-31T22:30:00Z"),
    )
    .await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            let held = NotifyStateRepo::list_held(&*harness.runtime.as_ref().unwrap().store)
                .await
                .unwrap();
            if !held.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(harness.notifier.sent().is_empty());
    assert_eq!(post_count(&harness.gateway), 0);

    harness.clock.set(at("2026-11-01T06:59:00Z"));
    harness.runtime.as_ref().unwrap().schedulers.minute().await;
    assert!(harness.notifier.sent().is_empty());
    harness.clock.set(at("2026-11-01T07:00:00Z"));
    harness.runtime.as_ref().unwrap().schedulers.minute().await;
    until(|| harness.notifier.sent().len() == 1).await;
    assert!(harness.notifier.sent()[0].markdown.contains("disk.full"));
    harness.stop().await;
}

#[tokio::test]
async fn heartbeat_failure_raises_advisory_and_success_clears_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let extra = "[heartbeat]\nurl = { env = \"HEARTBEAT_URL\" }\ninterval = \"1m\"\n[notify.digest]\nenabled = false";
    let mut harness =
        Harness::with_heartbeat(extra, at("2026-10-31T12:00:00Z"), &server.uri()).await;
    harness.start_schedulers();
    guard(async {
        loop {
            if !server.received_requests().await.unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;

    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500).set_delay(Duration::from_secs(60)))
        .mount(&server)
        .await;
    harness.clock.advance(SignedDuration::from_secs(670));
    // The ping now hangs, so only the fake clock can time it out. Keep moving the clock:
    // if the scheduler set its ping deadline after the advance above, a single advance
    // would never reach it.
    guard(async {
        loop {
            if harness
                .advisories()
                .await
                .iter()
                .any(|advisory| advisory.kind == AdvisoryKind::HeartbeatMissing)
            {
                break;
            }
            harness.clock.advance(SignedDuration::from_secs(5));
            tokio::task::yield_now().await;
        }
    })
    .await;

    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    harness.clock.advance(SignedDuration::from_mins(1));
    // As above: keep the clock moving until the next ping has gone out and succeeded.
    guard(async {
        loop {
            if !harness
                .advisories()
                .await
                .iter()
                .any(|advisory| advisory.kind == AdvisoryKind::HeartbeatMissing)
            {
                break;
            }
            harness.clock.advance(SignedDuration::from_secs(5));
            tokio::task::yield_now().await;
        }
    })
    .await;
    harness.stop().await;
}

#[derive(Default)]
struct LagRecorder {
    missed: Arc<AtomicU64>,
}

struct LagCounter(Arc<AtomicU64>);

impl CounterFn for LagCounter {
    fn increment(&self, value: u64) {
        self.0.fetch_add(value, Ordering::SeqCst);
    }

    fn absolute(&self, value: u64) {
        self.0.store(value, Ordering::SeqCst);
    }
}

impl Recorder for LagRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        if key.name() == "domovoi_event_bus_lagged_total" {
            Counter::from_arc(Arc::new(LagCounter(self.missed.clone())))
        } else {
            Counter::noop()
        }
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn bus_lag_triggers_reload_not_crash() {
    let mut harness = Harness::with_adapters(
        "",
        at("2026-10-31T12:00:00Z"),
        Arc::new(ScriptedLlm::new()),
        Vec::new(),
        1,
    )
    .await;
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        harness.webhook(&firing("web-2", "backup.failed")).await,
        StatusCode::ACCEPTED
    );
    let incidents = wait_for_incidents(&harness, 2).await;
    // An incident that opened and resolved before delivery caught up: history, not news.
    assert_eq!(
        harness.webhook(&firing("old-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        harness.webhook(&resolving("old-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            if harness
                .incidents()
                .await
                .iter()
                .any(|i| i.resolved_at().is_some())
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    harness
        .put_notify_state(
            incidents[0].id(),
            &NotifyState {
                messages: BTreeMap::from([(
                    "discord".to_owned(),
                    ChatRefs {
                        message: MessageRef {
                            platform: "discord".to_owned(),
                            id: "existing".to_owned(),
                        },
                        thread: None,
                    },
                )]),
                ..NotifyState::default()
            },
        )
        .await;

    let recorder = LagRecorder::default();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    harness.start_dispatcher();
    until(|| {
        recorder.missed.load(Ordering::SeqCst) > 0
            && harness
                .gateway
                .calls()
                .iter()
                .any(|call| matches!(call, GatewayCall::UpdateCard(message, _) if message.id == "existing"))
    })
    .await;
    assert_eq!(
        harness.webhook(&firing("web-3", "network.down")).await,
        StatusCode::ACCEPTED
    );
    until(|| post_count(&harness.gateway) >= 1).await;
    assert_eq!(wait_for_incidents(&harness, 4).await.len(), 4);
    assert!(
        !harness.gateway.calls().iter().any(|call| matches!(
            call,
            GatewayCall::PostCard(card) | GatewayCall::UpdateCard(_, card)
                if card.host.as_str() == "old-1"
        )),
        "recovery never posts or updates resolved incidents"
    );
    harness.stop().await;
}

#[tokio::test]
async fn quiet_hours_release_without_a_notifier_posts_held_cards() {
    let mut harness = Harness::without_notifier(
        "[notify]
quiet_hours = { start = \"22:00\", end = \"07:00\" }
[notify.digest]
enabled = false",
        at("2026-10-31T22:30:00Z"),
    )
    .await;
    harness.start_dispatcher();
    harness.start_schedulers();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            let held = NotifyStateRepo::list_held(&*harness.runtime.as_ref().unwrap().store)
                .await
                .unwrap();
            if !held.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(post_count(&harness.gateway), 0, "held during quiet hours");

    harness.clock.set(at("2026-11-01T07:00:00Z"));
    until(|| post_count(&harness.gateway) == 1).await;
    assert!(harness.gateway.calls().iter().any(|call| matches!(
        call,
        GatewayCall::PostCard(card) if card.host.as_str() == "web-1"
    )));
    harness.stop().await;
}

#[tokio::test]
async fn shutdown_completes_within_15_seconds_with_inflight_triage() {
    let provider = Arc::new(BlockingLlm::new());
    let started = provider.started.clone();
    let release = provider.releaser();
    let mut harness = Harness::with_adapters(
        "[triage]\nenabled = true\nconcurrency = 1",
        at("2026-10-31T12:00:00Z"),
        provider.clone(),
        Vec::new(),
        4096,
    )
    .await;
    let shutdown = CancellationToken::new();
    let runtime = harness.runtime.take().unwrap();
    let workers = runtime.worker_shutdown.clone();
    let mut app = runtime.app.clone();
    let running = tokio::spawn(runtime.run(shutdown.clone()));
    let body = firing("web-1", "disk.full");
    let mut mac =
        <hmac::Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(harness::SECRET.as_bytes())
            .unwrap();
    hmac::Mac::update(&mut mac, body.as_bytes());
    let signature = format!(
        "sha256={}",
        hex::encode(hmac::Mac::finalize(mac).into_bytes())
    );
    let request = axum::http::Request::post("/hooks/alerts")
        .header("x-domovoi-signature", signature)
        .body(axum::body::Body::from(body))
        .unwrap();
    assert_eq!(
        tower::ServiceExt::oneshot(&mut app, request)
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    guard(started.notified()).await;

    shutdown.cancel();
    guard(workers.cancelled_owned()).await;
    for _ in 0..5 {
        harness.clock.advance(SignedDuration::from_secs(1));
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }
    release.notify_one();
    let result = guard(running).await.unwrap();
    assert!(result.is_ok(), "shutdown failed: {result:?}");
    assert!(provider.completed());
    assert!(harness.clock.now() < at("2026-10-31T12:00:15Z"));
    harness.stop().await;
}
