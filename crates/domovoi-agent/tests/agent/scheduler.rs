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

use crate::harness::{ScriptedCheck, clock, empty_fs, firing, key, ok, report, settle};
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
        settings: Arc::new(BTreeMap::new()),
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
