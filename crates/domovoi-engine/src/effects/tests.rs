//! Runner behaviour over the in-memory store: claims, backoff, fencing and advisories.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{
    AdvisoryRepo, AggregateRef, ClaimBatch, ClaimToken, Clock, EffectDraft, EffectKind,
    EffectQueue, EffectRead, EffectRecord, Transaction,
};
use domovoi_core::{AdvisoryKind, EffectId, IncidentId};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

use super::{EffectHandler, EffectOutcome, EffectRunner, EffectRunnerDeps, EffectSettings};

fn deps(
    queue: Arc<dyn EffectQueue>,
    store: &Arc<InMemoryStore>,
    clock: &Arc<FakeClock>,
    nudge: &Arc<Notify>,
    shutdown: &CancellationToken,
) -> EffectRunnerDeps {
    EffectRunnerDeps {
        queue,
        uow: store.clone(),
        clock: clock.clone(),
        advisories: store.clone(),
        nudge: nudge.clone(),
        shutdown: shutdown.clone(),
    }
}

fn settings(poll: SignedDuration) -> EffectSettings {
    EffectSettings {
        lease: SignedDuration::from_mins(5),
        poll,
        batch: 8,
    }
}

/// Stores one pending effect and returns its id.
async fn seed(
    store: &InMemoryStore,
    kind: EffectKind,
    max_attempts: u32,
    not_before: Timestamp,
) -> EffectId {
    let draft = EffectDraft {
        id: EffectId::new(),
        kind,
        aggregate: AggregateRef::incident(IncidentId::new()),
        payload: serde_json::json!({"purpose": "test", "secret": "secret-payload"}),
        not_before,
        max_attempts,
        dedup_key: None,
        created_at: T0,
    };
    let id = draft.id;
    domovoi_core::ports::UnitOfWork::commit(store, Transaction::new().effect(draft))
        .await
        .expect("the effect is stored");
    id
}

/// Lets the runner make progress without moving the clock.
async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// Lets the runner make progress until `done` holds for the stored effect.
async fn until(
    store: &InMemoryStore,
    id: EffectId,
    done: impl Fn(&EffectRecord) -> bool,
) -> EffectRecord {
    for _ in 0..400 {
        settle().await;
        let effect = row(store, id).await;
        if done(&effect) {
            return effect;
        }
    }
    row(store, id).await
}

async fn row(store: &InMemoryStore, id: EffectId) -> EffectRecord {
    EffectRead::effect(store, id)
        .await
        .expect("the effect is readable")
        .expect("the effect exists")
}

#[derive(Default)]
struct RetryAlways {
    calls: AtomicU32,
}

#[async_trait]
impl EffectHandler for RetryAlways {
    async fn handle(&self, _effect: &EffectRecord, _claim: ClaimToken) -> EffectOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        EffectOutcome::Retry {
            error: "host is offline".to_owned(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_retries_with_backoff_then_dead_letters() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let id = seed(&store, EffectKind::Wake, 3, T0).await;
    let handler = Arc::new(RetryAlways::default());
    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, handler.clone());
    let task = tokio::spawn(runner.run());

    // Each attempt's retry is observed as the wait it left behind. The clock never moves
    // while a handler works, so the gap between one wait and the next is the backoff of the
    // attempt in between.
    let mut attempts: Vec<(u32, Timestamp)> = Vec::new();
    let mut dead = false;
    for _ in 0..600 {
        settle().await;
        let effect = row(&store, id).await;
        if effect.status == "pending"
            && effect.attempt > 0
            && attempts.last().map(|(attempt, _)| *attempt) != Some(effect.attempt)
        {
            attempts.push((effect.attempt, effect.not_before));
        }
        if effect.status == "dead" {
            dead = true;
            break;
        }
        if effect.status == "claimed" {
            continue;
        }
        if effect.not_before > Clock::now(&*clock) {
            clock.set(effect.not_before);
        } else {
            clock.advance(SignedDuration::from_secs(1));
        }
    }
    shutdown.cancel();
    task.await.expect("the runner stops");

    assert!(dead, "the effect is dead-lettered after its attempt limit");
    assert_eq!(
        attempts
            .iter()
            .map(|(attempt, _)| *attempt)
            .collect::<Vec<_>>(),
        [1, 2],
        "the two retries are recorded after their attempts"
    );
    let gap = attempts[1].1.duration_since(attempts[0].1);
    assert!(
        gap >= SignedDuration::from_secs(60) && gap < SignedDuration::from_secs(120),
        "the second retry waits twice as long as the first: {gap}"
    );
    let first = attempts[0].1.duration_since(T0);
    assert!(
        first >= SignedDuration::from_secs(30),
        "the first retry waits the base: {first}"
    );
    assert_eq!(handler.calls.load(Ordering::SeqCst), 3, "three attempts");
    let effect = row(&store, id).await;
    assert_eq!(effect.attempt, 3);
    assert!(
        effect
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("attempt limit")),
        "{:?}",
        effect.last_error
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_wakes_on_nudge_before_poll() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let queue = Arc::new(CountingQueue {
        store: store.clone(),
        calls: AtomicU32::new(0),
        signal: tx,
    });
    let runner = EffectRunner::new(
        deps(queue, &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_hours(1)),
    );
    let mut runner = runner;
    runner.register(EffectKind::Wake, Arc::new(RetryAlways::default()));
    let task = tokio::spawn(runner.run());

    rx.recv().await.expect("the first claim pass runs at once");
    nudge.notify_one();
    let woken = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await;
    shutdown.cancel();
    task.await.expect("the runner stops");
    assert!(
        woken.is_ok_and(|got| got.is_some()),
        "the nudge starts a claim pass long before the poll would"
    );
}

#[derive(Default)]
struct Gated {
    calls: AtomicU32,
    first: Notify,
    second: Notify,
}

#[async_trait]
impl EffectHandler for Gated {
    async fn handle(&self, _effect: &EffectRecord, _claim: ClaimToken) -> EffectOutcome {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => self.first.notified().await,
            _ => self.second.notified().await,
        }
        EffectOutcome::Retry {
            error: "host is offline".to_owned(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_discards_stale_handler_result() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let id = seed(&store, EffectKind::Wake, 5, T0).await;
    let handler = Arc::new(Gated::default());
    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, handler.clone());
    let task = tokio::spawn(runner.run());

    for _ in 0..200 {
        settle().await;
        if handler.calls.load(Ordering::SeqCst) == 1 {
            break;
        }
    }
    settle().await;
    let first = row(&store, id).await;
    assert_eq!(first.attempt, 1);

    clock.advance(SignedDuration::from_secs(301));
    for _ in 0..200 {
        settle().await;
        if handler.calls.load(Ordering::SeqCst) == 2 {
            break;
        }
    }
    settle().await;
    let second = row(&store, id).await;
    assert_eq!(second.attempt, 2, "the expired lease is reclaimed");
    assert_ne!(second.claim_token, first.claim_token);

    handler.first.notify_one();
    settle().await;
    let after = row(&store, id).await;
    assert_eq!(after.status, "claimed", "the stale result changed nothing");
    assert_eq!(after.claim_token, second.claim_token);
    assert_eq!(after.attempt, 2);

    handler.second.notify_one();
    shutdown.cancel();
    task.await.expect("the runner stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_reverts_claimed_effects_at_start() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let later = T0.checked_add(SignedDuration::from_hours(1)).unwrap_or(T0);
    let id = seed(&store, EffectKind::Wake, 5, later).await;
    store
        .claim_effect(
            id,
            ClaimToken::new(),
            T0.checked_add(SignedDuration::from_mins(5)).unwrap_or(T0),
        )
        .expect("the previous session's claim is stored");

    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, Arc::new(RetryAlways::default()));
    let task = tokio::spawn(runner.run());

    let effect = until(&store, id, |effect| effect.status == "pending").await;
    assert_eq!(effect.claim_generation, 2);
    assert_eq!(effect.claim_token, None);
    assert_eq!(effect.lease_until, None);
    assert_eq!(effect.runner_id, None);

    shutdown.cancel();
    task.await.expect("the runner stops");
}

struct Terminal;

#[async_trait]
impl EffectHandler for Terminal {
    async fn handle(&self, _effect: &EffectRecord, _claim: ClaimToken) -> EffectOutcome {
        EffectOutcome::Dead {
            error: "the payload is nonsense".to_owned(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dead_effect_raises_advisory() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let id = seed(&store, EffectKind::Wake, 3, T0).await;
    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, Arc::new(Terminal));
    let task = tokio::spawn(runner.run());

    let mut open = Vec::new();
    for _ in 0..600 {
        settle().await;
        open = AdvisoryRepo::list_open(&*store).await.unwrap_or_default();
        if !open.is_empty() {
            break;
        }
        clock.advance(SignedDuration::from_secs(1));
    }
    shutdown.cancel();
    task.await.expect("the runner stops");

    assert_eq!(row(&store, id).await.status, "dead");
    let advisory = open.first().expect("a dead effect raises one advisory");
    assert_eq!(advisory.kind, AdvisoryKind::EffectDead);
    assert_eq!(advisory.host, None);
    assert_eq!(
        advisory.title,
        "A background task failed permanently (wake)"
    );
    assert!(advisory.detail.contains("the payload is nonsense"));
    assert!(advisory.detail.contains("after 1 attempts"));
    assert!(
        !advisory.detail.contains("secret-payload"),
        "payload contents never reach the advisory"
    );
}

/// Counts claim passes and reports each one, so a test can see the runner wake.
struct CountingQueue {
    store: Arc<InMemoryStore>,
    calls: AtomicU32,
    signal: mpsc::UnboundedSender<()>,
}

#[async_trait]
impl EffectQueue for CountingQueue {
    async fn claim_due(
        &self,
        runner: &str,
        kinds: &[EffectKind],
        now: Timestamp,
        lease: SignedDuration,
        limit: u16,
    ) -> Result<ClaimBatch, domovoi_core::ports::StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.signal.send(());
        self.store.claim_due(runner, kinds, now, lease, limit).await
    }

    async fn revert_claimed_at_start(&self) -> Result<u64, domovoi_core::ports::StoreError> {
        self.store.revert_claimed_at_start().await
    }
}
