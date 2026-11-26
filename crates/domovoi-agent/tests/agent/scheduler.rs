//! The scheduler turns observations into firing and resolved signals and keeps state.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use domovoi_agent::checks::CheckSet;
use domovoi_agent::link::LinkHandle;
use domovoi_agent::scheduler::{CheckEnv, Scheduler};
use domovoi_agent::spool::{SharedSpool, Spool};
use domovoi_agent::store::CheckStore;
use domovoi_checks::commands::Commands;
use domovoi_checks::framework::{Check, CheckContext, CheckError, CheckReport, number_fact};
use domovoi_core::{CheckKey, HostId, Severity, SignalClass, SignalStatus};
use domovoi_proto::Message;
use domovoi_testkit::{FakeClock, FakeProcessExecutor};
use jiff::SignedDuration;
use tokio_util::sync::CancellationToken;

use crate::harness::{
    ScriptedCheck, block_spool, clock, empty_fs, firing, key, ok, report, settle, unblock_spool,
};
use crate::hub::guard;

struct Running {
    spool: Arc<SharedSpool>,
    store: Arc<CheckStore>,
    outbox: tokio::sync::mpsc::Receiver<domovoi_agent::link::Outgoing>,
    shutdown: CancellationToken,
    clock: Arc<FakeClock>,
    _dir: tempfile::TempDir,
}

fn start(checks: Vec<Box<dyn Check>>) -> Running {
    let dir = tempfile::tempdir().unwrap();
    let clock = clock();
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    let store = Arc::new(CheckStore::open(dir.path()).unwrap());
    let env = CheckEnv {
        host: HostId::new(),
        fs: empty_fs(&dir),
        exec: Arc::new(FakeProcessExecutor::new()),
        commands: Arc::new(Commands::default()),
        clock: clock.clone(),
        settings: domovoi_agent::overrides::fixed(BTreeMap::new()),
        privileged: None,
    };
    let (handle, outbox) = LinkHandle::channel(16);
    let checks: Arc<dyn CheckSet> = Arc::new(checks);
    let scheduler = Scheduler::new(checks, env, store.clone(), spool.clone(), handle);
    let shutdown = CancellationToken::new();
    tokio::spawn(scheduler.run(shutdown.clone()));
    Running {
        spool,
        store,
        outbox,
        shutdown,
        clock,
        _dir: dir,
    }
}

impl Running {
    /// Waits for the next spooled batch and returns `(target, status, severity)` per signal.
    async fn batch(&self) -> Vec<(String, SignalStatus, Severity)> {
        guard(self.spool.pushed()).await;
        let spool = self.spool.lock().await;
        let batch = spool.pending_after(None).pop().unwrap();
        batch
            .signals
            .iter()
            .map(|s| {
                (
                    s.target().map_or(String::new(), |t| t.to_string()),
                    s.status(),
                    s.severity(),
                )
            })
            .collect()
    }

    async fn next_run(&self) -> Vec<(String, SignalStatus, Severity)> {
        settle().await;
        self.clock.advance(SignedDuration::from_secs(60));
        self.batch().await
    }
}

#[tokio::test]
async fn scheduler_emits_resolved_when_target_recovers() {
    let check = ScriptedCheck::new(
        "test.check",
        vec![
            report(vec![
                firing("a", Severity::Warning),
                firing("b", Severity::Critical),
            ]),
            report(vec![ok("a"), firing("b", Severity::Critical)]),
            report(vec![ok("a")]),
            report(vec![ok("a")]),
        ],
    );
    let run = start(vec![Box::new(check)]);
    use SignalStatus::{Firing, Resolved};
    let a = || "a".to_owned();
    let b = || "b".to_owned();
    assert_eq!(
        run.batch().await,
        [
            (a(), Firing, Severity::Warning),
            (b(), Firing, Severity::Critical)
        ]
    );
    assert_eq!(
        run.next_run().await,
        [
            (a(), Resolved, Severity::Info),
            (b(), Firing, Severity::Critical)
        ]
    );
    // `b` disappeared from the report: one resolved signal.
    assert_eq!(run.next_run().await, [(b(), Resolved, Severity::Info)]);
    // Nothing fires and nothing was firing: no batch at all.
    settle().await;
    run.clock.advance(SignedDuration::from_secs(60));
    settle().await;
    assert_eq!(run.spool.lock().await.len(), 3);
    run.shutdown.cancel();
}

#[tokio::test]
async fn firing_signals_repeat_each_run() {
    let check = ScriptedCheck::new(
        "test.check",
        vec![report(vec![firing("a", Severity::Warning)])],
    );
    let run = start(vec![Box::new(check)]);
    let expected = [("a".to_owned(), SignalStatus::Firing, Severity::Warning)];
    assert_eq!(run.batch().await, expected);
    assert_eq!(run.next_run().await, expected);
    assert_eq!(run.next_run().await, expected);
    let seqs: Vec<u64> = run
        .spool
        .lock()
        .await
        .pending_after(None)
        .iter()
        .map(|b| b.batch_seq)
        .collect();
    assert_eq!(seqs, [0, 1, 2]);
    run.shutdown.cancel();
}

/// Holds its run open until released, counting how many run at once.
struct Gate {
    key: CheckKey,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    open: Arc<AtomicBool>,
}

#[async_trait]
impl Check for Gate {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_hours(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, _ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        while !self.open.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(report(vec![firing("x", Severity::Warning)]))
    }
}

#[tokio::test]
async fn scheduler_runs_at_most_four_checks_at_once() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let open = Arc::new(AtomicBool::new(false));
    let checks: Vec<Box<dyn Check>> = (0..6)
        .map(|i| {
            Box::new(Gate {
                key: key(&format!("gate.c{i}")),
                active: active.clone(),
                peak: peak.clone(),
                open: open.clone(),
            }) as Box<dyn Check>
        })
        .collect();
    let run = start(checks);
    guard(async {
        while active.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    settle().await;
    assert_eq!(active.load(Ordering::SeqCst), 4);
    open.store(true, Ordering::SeqCst);
    let batch = run.batch().await;
    assert_eq!(batch.len(), 6, "all six report in one batch");
    assert_eq!(peak.load(Ordering::SeqCst), 4);
    run.shutdown.cancel();
}

/// Reports one fact per run.
struct Facts;

#[async_trait]
impl Check for Facts {
    fn key(&self) -> CheckKey {
        key("test.facts")
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(10)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let now = ctx.clock.now();
        Ok(CheckReport {
            observations: Vec::new(),
            facts: vec![number_fact(
                ctx.host,
                "test.minute",
                "Minute",
                now.as_second() as f64 / 60.0,
                "",
                now,
            )],
        })
    }
}

#[tokio::test]
async fn digest_facts_publish_hourly() {
    let mut run = start(vec![Box::new(Facts)]);
    for _ in 0..6 {
        settle().await;
        run.clock.advance(SignedDuration::from_mins(10));
    }
    let out = guard(run.outbox.recv()).await.unwrap();
    let Message::DigestFacts(facts) = out.message else {
        panic!("expected digest facts");
    };
    // The latest value per key, once.
    assert_eq!(facts.facts.len(), 1);
    assert_eq!(facts.facts[0].key, "test.minute");
    assert!(run.outbox.try_recv().is_err(), "facts go out once an hour");
    run.shutdown.cancel();
}

#[tokio::test]
async fn check_state_persists_after_each_run() {
    let check = ScriptedCheck::new(
        "test.check",
        vec![report(vec![firing("a", Severity::Warning)])],
    );
    let run = start(vec![Box::new(check)]);
    run.batch().await;
    run.next_run().await;
    // The state is saved right after the batch is spooled.
    settle().await;
    let dir = run._dir.path().to_owned();
    let reopened = CheckStore::open(&dir).unwrap();
    assert_eq!(reopened.state(&key("test.check")).get("runs"), Some("2"));
    assert!(reopened.firing(&key("test.check")).contains("a"));
    assert!(run.store.firing(&key("test.check")).contains("a"));
    run.shutdown.cancel();
}

/// A check shared with the test, so its run count can be read while it is scheduled.
struct Shared(Arc<ScriptedCheck>);

#[async_trait]
impl Check for Shared {
    fn key(&self) -> CheckKey {
        self.0.key()
    }

    fn default_cadence(&self) -> SignedDuration {
        self.0.default_cadence()
    }

    fn class(&self) -> SignalClass {
        self.0.class()
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        self.0.run(ctx).await
    }
}

#[tokio::test]
async fn config_update_applies_check_overrides_live() {
    use domovoi_agent::overrides::EffectiveChecks;
    use domovoi_proto::messages::CheckOverride;

    let quiet = || (0..20).map(|_| report(vec![ok("x")])).collect::<Vec<_>>();
    let a = Arc::new(ScriptedCheck::new("test.a", quiet()));
    let b = Arc::new(ScriptedCheck::new("test.b", quiet()));
    let runs = |c: &Arc<ScriptedCheck>| c.runs.load(Ordering::SeqCst);

    let dir = tempfile::tempdir().unwrap();
    let clock = clock();
    let effective = EffectiveChecks::new(BTreeMap::new());
    let env = CheckEnv {
        host: HostId::new(),
        fs: empty_fs(&dir),
        exec: Arc::new(FakeProcessExecutor::new()),
        commands: Arc::new(Commands::default()),
        clock: clock.clone(),
        settings: effective.view(),
        privileged: None,
    };
    let spool = Arc::new(SharedSpool::new(Spool::open(dir.path()).unwrap()));
    let store = Arc::new(CheckStore::open(dir.path()).unwrap());
    let (handle, _outbox) = LinkHandle::channel(16);
    let checks: Arc<dyn CheckSet> = Arc::new(vec![
        Box::new(Shared(a.clone())) as Box<dyn Check>,
        Box::new(Shared(b.clone())),
    ]);
    let shutdown = CancellationToken::new();
    tokio::spawn(Scheduler::new(checks, env, store, spool, handle).run(shutdown.clone()));
    settle().await;
    assert_eq!((runs(&a), runs(&b)), (1, 1), "both run at start");

    effective.apply(&BTreeMap::from([
        (
            key("test.a"),
            CheckOverride {
                enabled: Some(false),
                ..CheckOverride::default()
            },
        ),
        (
            key("test.b"),
            CheckOverride {
                interval_seconds: Some(300),
                ..CheckOverride::default()
            },
        ),
    ]));
    settle().await;
    clock.advance(SignedDuration::from_secs(60));
    settle().await;
    assert_eq!(
        (runs(&a), runs(&b)),
        (1, 1),
        "a is off; b waits its new interval"
    );
    clock.advance(SignedDuration::from_secs(240));
    settle().await;
    assert_eq!(
        (runs(&a), runs(&b)),
        (1, 2),
        "b runs 300 s after its last run"
    );

    effective.apply(&BTreeMap::new());
    settle().await;
    assert_eq!(runs(&a), 2, "turned back on, a is overdue and runs at once");
    shutdown.cancel();
}

/// Waits until `counter` reaches `count`, so a test reads state only after the check ran.
async fn wait_for_runs(counter: &AtomicUsize, count: usize) {
    guard(async {
        while counter.load(Ordering::SeqCst) < count {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

/// Waits until the spool holds `count` batches.
async fn wait_for_batches(run: &Running, count: usize) {
    guard(async {
        while run.spool.lock().await.len() < count {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

/// Fires `a` on every run and counts its runs in its stored state, like a check that
/// remembers a cursor.
struct Counting {
    runs: Arc<AtomicUsize>,
}

#[async_trait]
impl Check for Counting {
    fn key(&self) -> CheckKey {
        key("test.counting")
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_secs(60)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let runs = ctx
            .state
            .get("runs")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        ctx.state.set("runs", runs.to_string());
        Ok(report(vec![firing("a", Severity::Warning)]))
    }
}

#[tokio::test]
async fn check_state_not_saved_when_spool_push_fails() {
    let runs = Arc::new(AtomicUsize::new(0));
    let run = start(vec![Box::new(Counting { runs: runs.clone() })]);
    let dir = run._dir.path().to_owned();
    block_spool(&dir);
    wait_for_runs(&runs, 1).await;
    settle().await;

    assert!(run.spool.lock().await.is_empty(), "no batch was added");
    assert_eq!(run.store.state(&key("test.counting")).get("runs"), None);
    assert!(run.store.firing(&key("test.counting")).is_empty());
    let reopened = CheckStore::open(&dir).unwrap();
    assert_eq!(reopened.state(&key("test.counting")).get("runs"), None);
    assert!(reopened.firing(&key("test.counting")).is_empty());

    unblock_spool(&dir);
    run.clock.advance(SignedDuration::from_secs(60));
    wait_for_runs(&runs, 2).await;
    wait_for_batches(&run, 1).await;

    assert_eq!(
        run.store.state(&key("test.counting")).get("runs"),
        Some("1"),
        "the run whose push failed was discarded, not remembered"
    );
    assert!(run.store.firing(&key("test.counting")).contains("a"));
    {
        let spool = run.spool.lock().await;
        let pending = spool.pending_after(None);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].signals.len(), 1);
        assert_eq!(pending[0].signals[0].status(), SignalStatus::Firing);
    }
    let reopened = CheckStore::open(&dir).unwrap();
    assert_eq!(reopened.state(&key("test.counting")).get("runs"), Some("1"));
    assert!(reopened.firing(&key("test.counting")).contains("a"));
    run.shutdown.cancel();
}

#[tokio::test]
async fn firing_signal_is_not_lost_when_spool_push_fails() {
    let runs = Arc::new(AtomicUsize::new(0));
    let run = start(vec![Box::new(Counting { runs: runs.clone() })]);
    let dir = run._dir.path().to_owned();
    block_spool(&dir);
    wait_for_runs(&runs, 1).await;
    settle().await;
    unblock_spool(&dir);
    run.clock.advance(SignedDuration::from_secs(60));
    wait_for_runs(&runs, 2).await;
    wait_for_batches(&run, 1).await;
    settle().await;

    let spool = run.spool.lock().await;
    let pending = spool.pending_after(None);
    assert_eq!(pending.len(), 1, "exactly one batch, not two");
    assert_eq!(pending[0].signals.len(), 1);
    assert_eq!(pending[0].signals[0].target().unwrap().as_str(), "a");
    assert_eq!(pending[0].signals[0].status(), SignalStatus::Firing);
    run.shutdown.cancel();
}

#[tokio::test]
async fn resolved_signal_is_not_lost_when_spool_push_fails() {
    let check = Arc::new(ScriptedCheck::new(
        "test.resolving",
        vec![
            report(vec![firing("a", Severity::Warning)]),
            report(vec![ok("a")]),
        ],
    ));
    let run = start(vec![Box::new(Shared(check.clone()))]);
    run.batch().await;
    settle().await;
    assert!(run.store.firing(&key("test.resolving")).contains("a"));

    let dir = run._dir.path().to_owned();
    block_spool(&dir);
    run.clock.advance(SignedDuration::from_secs(60));
    wait_for_runs(&check.runs, 2).await;
    settle().await;
    assert_eq!(run.spool.lock().await.len(), 1, "the resolved batch is not");
    assert!(
        run.store.firing(&key("test.resolving")).contains("a"),
        "the firing memory is not cleared by a run that could not be spooled"
    );

    unblock_spool(&dir);
    run.clock.advance(SignedDuration::from_secs(60));
    wait_for_runs(&check.runs, 3).await;
    wait_for_batches(&run, 2).await;

    let spool = run.spool.lock().await;
    let pending = spool.pending_after(None);
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[1].signals.len(), 1);
    assert_eq!(pending[1].signals[0].status(), SignalStatus::Resolved);
    assert_eq!(pending[1].signals[0].target().unwrap().as_str(), "a");
    assert!(run.store.firing(&key("test.resolving")).is_empty());
    run.shutdown.cancel();
}

#[tokio::test]
async fn check_with_no_signals_still_saves_state() {
    let check = Arc::new(ScriptedCheck::new(
        "test.quiet",
        vec![report(vec![ok("a")])],
    ));
    let run = start(vec![Box::new(Shared(check.clone()))]);
    wait_for_runs(&check.runs, 1).await;
    guard(async {
        while run.store.state(&key("test.quiet")).get("runs").is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await;

    assert!(run.spool.lock().await.is_empty(), "nothing to spool");
    assert_eq!(run.store.state(&key("test.quiet")).get("runs"), Some("1"));
    let reopened = CheckStore::open(run._dir.path()).unwrap();
    assert_eq!(reopened.state(&key("test.quiet")).get("runs"), Some("1"));
    run.shutdown.cancel();
}

/// Fails every run after writing to its state, as a check that hit a bad device does.
struct Failing;

#[async_trait]
impl Check for Failing {
    fn key(&self) -> CheckKey {
        key("test.failing")
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_secs(60)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        ctx.state.set("attempts", "1");
        Err(CheckError::Settings("no such device".to_owned()))
    }
}

#[tokio::test]
async fn failing_check_keeps_its_state_as_before() {
    let run = start(vec![Box::new(Failing)]);
    guard(async {
        while run
            .store
            .state(&key("test.failing"))
            .get("attempts")
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await;

    assert_eq!(
        run.store.state(&key("test.failing")).get("attempts"),
        Some("1")
    );
    assert!(
        run.spool.lock().await.is_empty(),
        "a failing check says nothing"
    );
    run.shutdown.cancel();
}

/// A check that reports one thing that happened, so a quiet run is no recovery.
struct EventCheck;

#[async_trait]
impl Check for EventCheck {
    fn key(&self) -> CheckKey {
        key("security.ssh_login")
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_secs(60)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Immediate
    }

    fn resolves_itself(&self) -> bool {
        false
    }

    async fn run(&self, _ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        Ok(CheckReport::default())
    }
}

#[test]
fn event_check_never_sends_a_recovery_while_a_state_check_does() {
    use std::collections::BTreeSet;

    use domovoi_agent::scheduler::signals_for;

    let now = jiff::Timestamp::UNIX_EPOCH;
    let previous = BTreeSet::from(["kuker".to_owned()]);
    let host = HostId::new();
    // Several quiet runs in a row: the event incident is never resolved by the agent.
    for _ in 0..3 {
        let (signals, _) = signals_for(host, &EventCheck, &report(vec![]), &previous, now);
        assert!(signals.is_empty(), "an event check sends no recovery");
        let quiet = report(vec![ok("kuker")]);
        let (signals, _) = signals_for(host, &EventCheck, &quiet, &previous, now);
        assert!(
            signals.is_empty(),
            "an observation that is fine is no recovery either"
        );
    }
    // A state check resolves the same way it always did.
    let state = ScriptedCheck::new("disk.free", vec![]);
    let (signals, _) = signals_for(host, &state, &report(vec![]), &previous, now);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].status(), SignalStatus::Resolved);
    let (signals, _) = signals_for(host, &state, &report(vec![ok("kuker")]), &previous, now);
    assert_eq!(signals[0].status(), SignalStatus::Resolved);
}
