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

use super::runner::Running;
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
            // The runner has claimed the effect and is committing the retry. Let it
            // finish before deciding whether the clock may move; advancing now would
            // change the retry's backoff base.
            for _ in 0..100 {
                settle().await;
                if row(&store, id).await.status != "claimed" {
                    break;
                }
            }
            continue;
        }
        if effect.not_before > Clock::now(&*clock) {
            clock.set(effect.not_before);
        } else {
            clock.advance(SignedDuration::from_secs(1));
        }
        // The runner may be waiting on its poll timer rather than the clock change; the
        // nudge is the test's deterministic way to wake it.
        nudge.notify_one();
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

struct Gated {
    calls: AtomicU32,
    /// Fires once per handler entry, so a test can wait for an attempt instead of polling.
    entered: mpsc::UnboundedSender<()>,
    first: Notify,
    second: Notify,
}

#[async_trait]
impl EffectHandler for Gated {
    async fn handle(&self, _effect: &EffectRecord, _claim: ClaimToken) -> EffectOutcome {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        // Report the entry before blocking, so a test waits for this handler instead of
        // polling for it: the claim has happened by the time the signal arrives.
        let _ = self.entered.send(());
        match call {
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
    let (entered, mut entries) = mpsc::unbounded_channel();
    let handler = Arc::new(Gated {
        calls: AtomicU32::new(0),
        entered,
        first: Notify::new(),
        second: Notify::new(),
    });
    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, handler.clone());
    let task = tokio::spawn(runner.run());

    entries
        .recv()
        .await
        .expect("the first claim reaches the handler");
    let first = row(&store, id).await;
    assert_eq!(first.attempt, 1);

    clock.advance(SignedDuration::from_secs(301));
    entries
        .recv()
        .await
        .expect("the expired lease is reclaimed");
    let second = row(&store, id).await;
    assert_eq!(second.attempt, 2, "the expired lease is reclaimed");
    assert_ne!(second.claim_token, first.claim_token);

    // The first holder now reports a retry it no longer owns: nothing may change while its
    // fenced commit is refused.
    handler.first.notify_one();
    for _ in 0..200 {
        settle().await;
        let after = row(&store, id).await;
        assert_eq!(after.status, "claimed", "the stale result changed nothing");
        assert_eq!(after.claim_token, second.claim_token);
        assert_eq!(after.attempt, 2);
        clock.advance(SignedDuration::from_secs(1));
    }

    handler.second.notify_one();
    shutdown.cancel();
    task.await.expect("the runner stops");
}

/// Two tasks for the same effect are tracked separately, so one finishing never forgets the
/// other: a handler that outlives its lease leaves two generations in flight at once.
#[tokio::test]
async fn two_generations_of_one_effect_are_tracked_separately() {
    let older = tokio::spawn(std::future::pending::<()>());
    let newer = tokio::spawn(std::future::pending::<()>());
    let effect = EffectId::new();
    let mut running = Running::default();
    running.start(older.id(), effect);
    running.start(newer.id(), effect);
    assert_eq!(running.len(), 2);

    assert_eq!(
        running.finish(older.id()),
        Some(effect),
        "the older task is named by its own effect"
    );
    assert_eq!(
        running.len(),
        1,
        "the newer generation keeps its tracking when the older one finishes"
    );
    assert_eq!(running.finish(older.id()), None, "a task is forgotten once");
    assert_eq!(
        running.finish(newer.id()),
        Some(effect),
        "the newer task is named by its own effect, which is what a panic log reports"
    );
    assert_eq!(running.len(), 0);

    older.abort();
    newer.abort();
}

/// A handler that keeps running after its lease expires reports two generations in flight.
struct Overlapping {
    calls: AtomicU32,
    entered: mpsc::UnboundedSender<()>,
    older: Notify,
    newer: Notify,
}

#[async_trait]
impl EffectHandler for Overlapping {
    async fn handle(&self, _effect: &EffectRecord, _claim: ClaimToken) -> EffectOutcome {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.entered.send(());
        if call == 0 {
            self.older.notified().await;
            EffectOutcome::Retry {
                error: "host is offline".to_owned(),
            }
        } else {
            self.newer.notified().await;
            panic!("the reclaimed generation panicked");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_generation_does_not_disturb_the_current_one() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let id = seed(&store, EffectKind::Wake, 5, T0).await;
    let (entered, mut entries) = mpsc::unbounded_channel();
    let handler = Arc::new(Overlapping {
        calls: AtomicU32::new(0),
        entered,
        older: Notify::new(),
        newer: Notify::new(),
    });
    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, handler.clone());
    let task = tokio::spawn(runner.run());

    entries.recv().await.expect("the first generation starts");
    let first = row(&store, id).await;
    assert_eq!(first.attempt, 1);

    clock.advance(SignedDuration::from_secs(301));
    entries
        .recv()
        .await
        .expect("the reclaimed generation starts");
    let second = row(&store, id).await;
    assert_eq!(second.attempt, 2);
    assert_ne!(second.claim_token, first.claim_token);

    // The older generation now reports a retry it no longer owns, while the newer one is
    // still in flight: only the stale holder's own commit is refused.
    handler.older.notify_one();
    for _ in 0..100 {
        settle().await;
    }
    let after = row(&store, id).await;
    assert_eq!(after.status, "claimed");
    assert_eq!(after.claim_token, second.claim_token);
    assert_eq!(after.attempt, 2);

    // The newer generation panics: the effect stays claimed under its token, and the runner
    // survives the task.
    handler.newer.notify_one();
    for _ in 0..100 {
        settle().await;
    }
    let after_panic = row(&store, id).await;
    assert_eq!(after_panic.status, "claimed");
    assert_eq!(after_panic.claim_token, second.claim_token);
    assert_eq!(after_panic.attempt, 2);
    assert!(after_panic.completed_at.is_none());

    shutdown.cancel();
    task.await
        .expect("the runner stops after a generation panicked");
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

/// Panics every time it runs, so the effect can only leave `claimed` by its lease expiring.
#[derive(Default)]
struct Panics {
    calls: AtomicU32,
}

#[async_trait]
impl EffectHandler for Panics {
    async fn handle(&self, _effect: &EffectRecord, _claim: ClaimToken) -> EffectOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("the handler blew up");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handler_panic_leaves_the_effect_claimed_and_the_runner_running() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let panicking = seed(&store, EffectKind::Wake, 3, T0).await;
    let other = seed(&store, EffectKind::Notify, 3, T0).await;
    let handler = Arc::new(Panics::default());
    let mut runner = EffectRunner::new(
        deps(store.clone(), &store, &clock, &nudge, &shutdown),
        settings(SignedDuration::from_secs(1)),
    );
    runner.register(EffectKind::Wake, handler.clone());
    runner.register(EffectKind::Notify, Arc::new(Terminal));
    let task = task_of(runner);

    // The other kind is still processed in the same pass as the panic: the runner survives it.
    let mut dead = false;
    for _ in 0..600 {
        settle().await;
        if row(&store, other).await.status == "dead" {
            dead = true;
            break;
        }
        clock.advance(SignedDuration::from_secs(1));
    }
    assert!(dead, "the runner kept claiming after a handler panicked");
    // The panicking task may be polled after its sibling finished, so wait for it to run.
    for _ in 0..200 {
        settle().await;
        if handler.calls.load(Ordering::SeqCst) >= 1 {
            break;
        }
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);

    let claimed = row(&store, panicking).await;
    assert_eq!(
        claimed.status, "claimed",
        "a panic never marks the effect done"
    );
    assert_eq!(claimed.attempt, 1);
    assert!(claimed.completed_at.is_none());

    // Its lease expiry is the only way back: the next pass reclaims it under a new token.
    clock.advance(SignedDuration::from_secs(301));
    let mut reclaimed = false;
    for _ in 0..600 {
        settle().await;
        if row(&store, panicking).await.attempt == 2 && handler.calls.load(Ordering::SeqCst) >= 2 {
            reclaimed = true;
            break;
        }
        clock.advance(SignedDuration::from_secs(1));
    }
    assert!(
        reclaimed,
        "the lease expiry made the panicked effect claimable"
    );
    assert_eq!(
        row(&store, panicking).await.status,
        "claimed",
        "the second attempt panicked too, so the effect is claimed again"
    );
    assert!(row(&store, panicking).await.completed_at.is_none());

    shutdown.cancel();
    task.await
        .expect("the runner stops after its handler panicked");
}

/// Runs a runner as a task the way the action service does.
fn task_of(runner: EffectRunner) -> tokio::task::JoinHandle<()> {
    tokio::spawn(runner.run())
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

/// Seeds `count` effects that have already used their only attempt and whose lease has run
/// out, so the next claim pass dead-letters them.
async fn exhausted(store: &InMemoryStore, clock: &FakeClock, count: usize) -> Vec<EffectId> {
    let mut ids = Vec::new();
    for _ in 0..count {
        ids.push(seed(store, EffectKind::Wake, 1, T0).await);
    }
    let claimed = store
        .claim_due(
            "seeder",
            &[EffectKind::Wake],
            T0,
            SignedDuration::from_secs(5),
            10,
        )
        .await
        .expect("the seeder claims them");
    assert_eq!(claimed.claimed.len(), count);
    clock.set(T0.checked_add(SignedDuration::from_secs(6)).unwrap_or(T0));
    ids
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batch_of_dead_effects_counts_as_full() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let ids = exhausted(&store, &clock, 3).await;
    let (tx, mut passes) = mpsc::unbounded_channel();
    let queue = Arc::new(CountingQueue {
        store: store.clone(),
        calls: AtomicU32::new(0),
        signal: tx,
    });
    let mut runner = EffectRunner::new(
        deps(queue, &store, &clock, &nudge, &shutdown),
        EffectSettings {
            lease: SignedDuration::from_mins(5),
            // A poll this long can only wake on a nudge: a second pass means the batch was
            // treated as full.
            poll: SignedDuration::from_hours(1),
            batch: 2,
        },
    );
    runner.register(EffectKind::Wake, Arc::new(Terminal));
    let task = tokio::spawn(runner.run());

    passes.recv().await.expect("the first claim pass runs");
    let second = tokio::time::timeout(std::time::Duration::from_secs(5), passes.recv()).await;
    assert!(
        second.is_ok_and(|got| got.is_some()),
        "two dead effects fill a batch of two, so the runner claims again at once"
    );

    let mut dead = false;
    for _ in 0..600 {
        settle().await;
        let mut all = true;
        for id in &ids {
            all &= row(&store, *id).await.status == "dead";
        }
        if all {
            dead = true;
            break;
        }
    }
    assert!(dead, "the second pass dead-letters the third effect");

    shutdown.cancel();
    task.await.expect("the runner stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mixed_batch_counts_every_candidate() {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let nudge = Arc::new(Notify::new());
    let shutdown = CancellationToken::new();
    let spent = exhausted(&store, &clock, 1).await;
    let pending = seed(&store, EffectKind::Wake, 3, T0).await;
    let (tx, mut passes) = mpsc::unbounded_channel();
    let queue = Arc::new(CountingQueue {
        store: store.clone(),
        calls: AtomicU32::new(0),
        signal: tx,
    });
    let mut runner = EffectRunner::new(
        deps(queue, &store, &clock, &nudge, &shutdown),
        EffectSettings {
            lease: SignedDuration::from_mins(5),
            poll: SignedDuration::from_hours(1),
            batch: 2,
        },
    );
    runner.register(EffectKind::Wake, Arc::new(Terminal));
    let task = tokio::spawn(runner.run());

    passes.recv().await.expect("the first claim pass runs");
    let second = tokio::time::timeout(std::time::Duration::from_secs(5), passes.recv()).await;
    assert!(
        second.is_ok_and(|got| got.is_some()),
        "one dead-lettered and one claimed effect fill a batch of two"
    );

    let mut settled = false;
    for _ in 0..600 {
        settle().await;
        if row(&store, spent[0]).await.status == "dead"
            && row(&store, pending).await.status == "dead"
        {
            settled = true;
            break;
        }
    }
    assert!(
        settled,
        "the exhausted effect is dead-lettered and the pending one handled"
    );

    shutdown.cancel();
    task.await.expect("the runner stops");
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
