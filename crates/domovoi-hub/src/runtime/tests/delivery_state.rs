//! Notification state is written only after a delivery was confirmed, and a state that
//! cannot be read is an error rather than an empty state.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::http::StatusCode;
use domovoi_core::ports::{
    ChatError, ChatGateway, InteractionSink, MessageRef, NotifyStateRepo, SettingsRepo, ThreadRef,
};
use domovoi_core::{AdvisoryKind, Card, IncidentId, NotifyState};
use domovoi_testkit::{GatewayCall, RecordingGateway, ScriptedLlm};
use jiff::SignedDuration;
use metrics::{
    Counter, CounterFn, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
};

use super::harness::{Harness, guard, until};
use super::{at, firing, post_count, resolving, wait_for_incidents};

/// The notification state the store holds for `incident`, or `None` when there is no row.
async fn stored_state(harness: &Harness, incident: IncidentId) -> Option<NotifyState> {
    NotifyStateRepo::get(&*harness.runtime.as_ref().unwrap().store, incident)
        .await
        .unwrap()
}

/// Every incident waiting for quiet hours to end.
async fn held(harness: &Harness) -> Vec<(IncidentId, NotifyState)> {
    NotifyStateRepo::list_held(&*harness.runtime.as_ref().unwrap().store)
        .await
        .unwrap()
}

/// A warning signal body, then the same check again at critical severity.
fn critical(host: &str, check: &str) -> String {
    format!(
        r#"{{"host":"{host}","check":"{check}","severity":"critical","summary":"{check} failed"}}"#
    )
}

/// How many cards a gateway was asked to post, failed attempts included.
fn post_attempts(gateway: &RecordingGateway) -> usize {
    gateway
        .calls()
        .iter()
        .filter(|call| matches!(call, GatewayCall::PostCard(_)))
        .count()
}

/// How many times a gateway opened a thread, which only happens after a card was posted.
fn thread_attempts(gateway: &RecordingGateway) -> usize {
    gateway
        .calls()
        .iter()
        .filter(|call| matches!(call, GatewayCall::OpenThread(_, _)))
        .count()
}

fn reply_attempts(gateway: &RecordingGateway) -> usize {
    gateway
        .calls()
        .iter()
        .filter(|call| matches!(call, GatewayCall::Reply(_, _)))
        .count()
}

/// Advances the fake clock one second at a time while `done` is false, letting retries run.
async fn advance_until(harness: &Harness, mut done: impl FnMut() -> bool) {
    guard(async {
        while !done() {
            harness.clock.advance(SignedDuration::from_secs(1));
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;
}

/// Advances the fake clock one second at a time until a delivery has failed for good.
async fn until_delivery_failed(harness: &Harness) {
    guard(async {
        loop {
            if harness
                .advisories()
                .await
                .iter()
                .any(|advisory| advisory.kind == AdvisoryKind::DeliveryFailed)
            {
                return;
            }
            harness.clock.advance(SignedDuration::from_secs(1));
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;
}

#[tokio::test]
async fn notification_state_written_only_after_success() {
    let bad = Arc::new(RecordingGateway::new("bad"));
    // One full retry chain: the escalation below must find the gateway healthy again.
    for _ in 0..6 {
        bad.fail_next(ChatError::Transport("offline".to_owned()));
    }
    let mut harness = Harness::with_adapters(
        "",
        at("2026-10-31T12:00:00Z"),
        Arc::new(ScriptedLlm::new()),
        vec![bad.clone()],
        4096,
    )
    .await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    let incident = wait_for_incidents(&harness, 1).await.remove(0);
    advance_until(&harness, || post_attempts(&bad) == 6).await;

    if let Some(state) = stored_state(&harness, incident.id()).await {
        assert_eq!(
            state.first_alert_at, None,
            "a delivery that failed is not recorded as sent"
        );
        assert!(state.messages.is_empty());
        assert!(!state.is_held());
    }

    // The alert resolves and fires again, so the reopening plans a card from the state the
    // failed delivery never advanced, and this time it is confirmed and stored.
    assert_eq!(
        harness.webhook(&resolving("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            if thread_attempts(&bad) == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    guard(async {
        loop {
            let stored = stored_state(&harness, incident.id())
                .await
                .unwrap_or_default();
            if stored.first_alert_at.is_some() && stored.messages.contains_key("bad") {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(thread_attempts(&bad), 1, "exactly one card went out");
    harness.stop().await;
}

#[tokio::test]
async fn notification_state_written_after_a_confirmed_post() {
    let mut harness = Harness::new("", at("2026-10-31T12:00:00Z")).await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    until(|| post_count(&harness.gateway) == 1).await;
    let incident = wait_for_incidents(&harness, 1).await.remove(0);
    guard(async {
        loop {
            let stored = stored_state(&harness, incident.id())
                .await
                .unwrap_or_default();
            if stored.first_alert_at.is_some() && stored.messages.contains_key("discord") {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    harness.stop().await;
}

/// Posts cards and opens no thread for its first six attempts, then behaves, so a delivery
/// that failed its thread does not leave a retry chain asleep when the test stops the hub.
#[derive(Default)]
struct ThreadlessGateway {
    posts: AtomicUsize,
    threads: AtomicUsize,
    replies: AtomicUsize,
    updates: AtomicUsize,
}

#[async_trait]
impl ChatGateway for ThreadlessGateway {
    fn platform(&self) -> &str {
        "discord"
    }

    async fn post_card(&self, _card: &Card) -> Result<MessageRef, ChatError> {
        self.posts.fetch_add(1, Ordering::SeqCst);
        Ok(MessageRef {
            platform: "discord".to_owned(),
            id: "card-1".to_owned(),
        })
    }

    async fn update_card(&self, _msg: &MessageRef, _card: &Card) -> Result<(), ChatError> {
        self.updates.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn open_thread(&self, msg: &MessageRef, _title: &str) -> Result<ThreadRef, ChatError> {
        // One whole retry chain fails, which is what the test is about.
        if self.threads.fetch_add(1, Ordering::SeqCst) < 6 {
            return Err(ChatError::Transport("no threads here".to_owned()));
        }
        Ok(ThreadRef {
            platform: "discord".to_owned(),
            id: msg.id.clone(),
        })
    }

    async fn reply(&self, _thread: &ThreadRef, _text: &str) -> Result<(), ChatError> {
        if self.replies.fetch_add(1, Ordering::SeqCst) < 6 {
            return Err(ChatError::Transport("no threads here".to_owned()));
        }
        Ok(())
    }

    async fn run(&self, _sink: Arc<dyn InteractionSink>) -> Result<(), ChatError> {
        Ok(())
    }
}

#[tokio::test]
async fn card_reference_saved_when_thread_fails_and_not_reposted() {
    let gateway = Arc::new(ThreadlessGateway::default());
    let mut harness = Harness::with_gateways(
        "",
        at("2026-10-31T12:00:00Z"),
        vec![gateway.clone() as Arc<dyn ChatGateway>],
    )
    .await;
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    let incident = wait_for_incidents(&harness, 1).await.remove(0);
    advance_until(&harness, || gateway.posts.load(Ordering::SeqCst) == 1).await;
    advance_until(&harness, || gateway.threads.load(Ordering::SeqCst) == 6).await;

    let stored = stored_state(&harness, incident.id())
        .await
        .unwrap_or_default();
    let refs = stored
        .messages
        .get("discord")
        .expect("a posted card is remembered at once");
    assert_eq!(refs.message.id, "card-1");
    assert!(refs.thread.is_none(), "the thread was never opened");
    assert_eq!(
        stored.first_alert_at, None,
        "a failed thread does not advance the policy state"
    );

    // A later event finds the saved reference and does not post the card again.
    assert_eq!(
        harness.webhook(&critical("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    advance_until(&harness, || gateway.updates.load(Ordering::SeqCst) == 1).await;
    assert_eq!(
        gateway.posts.load(Ordering::SeqCst),
        1,
        "no second card after the reference was saved"
    );
    harness.stop().await;
}

/// Records `domovoi_notify_state_errors_total`, split by its `op` label.
#[derive(Default)]
struct StateRecorder {
    reads: Arc<AtomicU64>,
    writes: Arc<AtomicU64>,
}

struct OpCounter(Arc<AtomicU64>);

impl CounterFn for OpCounter {
    fn increment(&self, value: u64) {
        self.0.fetch_add(value, Ordering::SeqCst);
    }

    fn absolute(&self, value: u64) {
        self.0.store(value, Ordering::SeqCst);
    }
}

impl Recorder for StateRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        if key.name() != "domovoi_notify_state_errors_total" {
            return Counter::noop();
        }
        // `op` is the only label: `read` counts reads, anything else counts writes.
        let counter = if key
            .labels()
            .any(|label| label.key() == "op" && label.value() == "read")
        {
            self.reads.clone()
        } else {
            self.writes.clone()
        };
        Counter::from_arc(Arc::new(OpCounter(counter)))
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::noop()
    }

    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}

#[tokio::test]
async fn notification_state_read_error_is_not_default_state() {
    let mut harness = Harness::new("", at("2026-10-31T12:00:00Z")).await;
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    let incident = wait_for_incidents(&harness, 1).await.remove(0);
    let store = harness.runtime.as_ref().unwrap().store.clone();
    sqlx::query(
        "INSERT INTO notify_state (incident_id, held, state, updated_at) \
         VALUES (?, 0, 'not json', 0)",
    )
    .bind(incident.id().to_string())
    .execute(store.pool_for_tests())
    .await
    .unwrap();

    let recorder = StateRecorder::default();
    let reads = recorder.reads.clone();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    harness.start_dispatcher();
    assert_eq!(
        harness.webhook(&critical("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            if reads.load(Ordering::SeqCst) > 0 || post_count(&harness.gateway) > 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;

    assert_eq!(
        post_count(&harness.gateway),
        0,
        "a state read error must not deliver a card planned from a default state"
    );
    assert!(
        reads.load(Ordering::SeqCst) > 0,
        "the read error is counted"
    );
    let (state,): (String,) =
        sqlx::query_as("SELECT state FROM notify_state WHERE incident_id = ?")
            .bind(incident.id().to_string())
            .fetch_one(store.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(state, "not json", "the row is left alone");
    harness.stop().await;
}

#[tokio::test]
async fn quiet_hours_release_not_saved_before_delivery() {
    let mut harness = Harness::without_notifier(
        "[notify]
quiet_hours = { start = \"22:00\", end = \"07:00\" }
[notify.digest]
enabled = false",
        at("2026-10-31T22:30:00Z"),
    )
    .await;
    for _ in 0..6 {
        harness
            .gateway
            .fail_next(ChatError::Transport("offline".to_owned()));
    }
    harness.start_dispatcher();
    harness.start_schedulers();
    assert_eq!(
        harness.webhook(&firing("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    guard(async {
        loop {
            if !held(&harness).await.is_empty() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;

    harness.clock.set(at("2026-11-01T07:00:00Z"));
    advance_until(&harness, || post_attempts(&harness.gateway) == 6).await;
    assert!(
        !held(&harness).await.is_empty(),
        "a release whose card was not delivered stays held"
    );

    harness.runtime.as_ref().unwrap().schedulers.minute().await;
    // The release is handed to the dispatcher; wait for the delivery attempt and for the
    // held card to clear, rather than sampling either once.
    guard(async {
        loop {
            if held(&harness).await.is_empty() && post_attempts(&harness.gateway) >= 7 {
                break;
            }
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;
    let incident = harness.incidents().await.remove(0);
    let stored = stored_state(&harness, incident.id())
        .await
        .unwrap_or_default();
    assert!(stored.messages.contains_key("discord"));
    harness.stop().await;
}

#[tokio::test]
async fn reminder_state_not_advanced_when_reminder_fails() {
    let mut harness = Harness::new("", at("2026-10-31T12:00:00Z")).await;
    harness.start_dispatcher();
    harness.start_schedulers();
    assert_eq!(
        harness.webhook(&critical("web-1", "disk.full")).await,
        StatusCode::ACCEPTED
    );
    let incident = wait_for_incidents(&harness, 1).await.remove(0);
    guard(async {
        loop {
            let stored = stored_state(&harness, incident.id())
                .await
                .unwrap_or_default();
            if stored.messages.values().any(|refs| refs.thread.is_some()) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;

    // Critical incidents are reminded every four hours; the reply fails every attempt.
    for _ in 0..6 {
        harness
            .gateway
            .fail_next(ChatError::Transport("offline".to_owned()));
    }
    harness.clock.set(at("2026-10-31T16:05:00Z"));
    advance_until(&harness, || reply_attempts(&harness.gateway) == 6).await;
    let stored = stored_state(&harness, incident.id())
        .await
        .unwrap_or_default();
    assert_eq!(
        stored.last_reminder_at, None,
        "a reminder that was not delivered is not recorded"
    );

    // The next tick plans it again, because the state did not move.
    harness.clock.set(at("2026-10-31T16:06:00Z"));
    guard(async {
        loop {
            let stored = stored_state(&harness, incident.id())
                .await
                .unwrap_or_default();
            if stored.last_reminder_at.is_some() {
                return;
            }
            harness.clock.advance(SignedDuration::from_secs(1));
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;
    harness.stop().await;
}

#[tokio::test]
async fn summary_failure_keeps_held_incident_held_until_a_card_is_delivered() {
    let mut harness = Harness::new(
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
            if !held(&harness).await.is_empty() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;

    // The summary goes out on its first retry; the card that follows it never does.
    harness
        .notifier
        .fail_next(ChatError::Transport("offline".to_owned()));
    for _ in 0..6 {
        harness
            .gateway
            .fail_next(ChatError::Transport("offline".to_owned()));
    }
    harness.clock.set(at("2026-11-01T07:00:00Z"));
    advance_until(&harness, || post_attempts(&harness.gateway) == 6).await;
    assert!(
        !held(&harness).await.is_empty(),
        "the fallback card was not delivered, so the alert stays held"
    );

    // Let the scheduler's own next ticks retry the release. Driving it through the clock
    // keeps one tick in flight at a time; calling `minute()` here would race the scheduled
    // tick and a late failure could re-hold the incident after it was released.
    guard(async {
        loop {
            if held(&harness).await.is_empty() {
                return;
            }
            harness.clock.advance(SignedDuration::from_secs(30));
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }
    })
    .await;
    assert!(
        held(&harness).await.is_empty(),
        "released once the card went out"
    );
    let incident = harness.incidents().await.remove(0);
    let stored = stored_state(&harness, incident.id())
        .await
        .unwrap_or_default();
    assert!(stored.messages.contains_key("discord"));
    harness.stop().await;
}

#[tokio::test]
async fn digest_delivery_date_is_saved_only_after_a_notifier_accepts_it() {
    let mut harness = Harness::new(
        "[notify.digest]\nenabled = true\ntime = \"09:00\"",
        at("2026-10-31T08:59:00Z"),
    )
    .await;
    harness.start_schedulers();
    for _ in 0..6 {
        harness
            .notifier
            .fail_next(ChatError::Transport("offline".to_owned()));
    }
    harness.clock.set(at("2026-10-31T09:00:00Z"));
    until_delivery_failed(&harness).await;

    assert!(harness.notifier.sent().is_empty());
    let stored = SettingsRepo::get(
        &*harness.runtime.as_ref().unwrap().store,
        "digest_last_sent_at",
    )
    .await
    .unwrap();
    assert!(
        stored.is_none(),
        "a digest every notifier refused is not recorded as sent"
    );
    harness.stop().await;
}
