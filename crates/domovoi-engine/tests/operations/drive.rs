//! Driving a running operation service: a gate on the runner's claims, canned agent answers,
//! and loops that let the runner and the writer work against the fake clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateKind, CheckRecheck, ClaimBatch, EffectKind, EffectQueue, SkillDispatch, SkillOutput,
    StoreError,
};
use domovoi_core::{
    Evidence, JournalEpoch, OperationId, OperationSnapshot, SignalStatus, StepKind, StepOutcome,
    VerifySpec,
};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::T0;
use domovoi_testkit::link::LinkRequest;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

use crate::harness::{World, host_id, restart_skill};

/// The effect queue the runner claims from, which can be closed so it claims nothing.
pub struct Gated {
    inner: Arc<InMemoryStore>,
    closed: AtomicBool,
}

impl Gated {
    /// A gate over `inner`, closed or open.
    pub fn new(inner: Arc<InMemoryStore>, closed: bool) -> Arc<Self> {
        Arc::new(Self {
            inner,
            closed: AtomicBool::new(closed),
        })
    }

    /// Lets the runner claim again.
    pub fn open(&self) {
        self.closed.store(false, Ordering::SeqCst);
    }

    /// Makes the runner claim nothing from now on.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl EffectQueue for Gated {
    async fn claim_due(
        &self,
        runner: &str,
        kinds: &[EffectKind],
        aggregates: &[AggregateKind],
        now: Timestamp,
        lease: SignedDuration,
        limit: u16,
    ) -> Result<ClaimBatch, StoreError> {
        if self.closed.load(Ordering::SeqCst) {
            return Ok(ClaimBatch::default());
        }
        self.inner
            .claim_due(runner, kinds, aggregates, now, lease, limit)
            .await
    }

    async fn revert_claimed_at_start(
        &self,
        aggregates: &[AggregateKind],
    ) -> Result<u64, StoreError> {
        self.inner.revert_claimed_at_start(aggregates).await
    }
}

/// A skill run that exited 0.
pub fn ok_output(stdout: &str) -> SkillOutput {
    SkillOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        stderr: String::new(),
        truncated: false,
        duration_ms: 1200,
        argv_hash: [0xab; 32],
    }
}

/// A skill run that exited with `code`.
pub fn failed_output(code: i32) -> SkillOutput {
    SkillOutput {
        exit_code: Some(code),
        stderr: "no such container".to_owned(),
        ..ok_output("")
    }
}

/// A re-check that found the condition gone.
pub fn resolved() -> CheckRecheck {
    recheck(SignalStatus::Resolved)
}

/// A re-check that found the condition still there.
pub fn firing() -> CheckRecheck {
    recheck(SignalStatus::Firing)
}

fn recheck(status: SignalStatus) -> CheckRecheck {
    CheckRecheck {
        status,
        summary: "plex is running".to_owned(),
        evidence: Evidence::new(),
        observed_at: T0,
    }
}

/// A fresh journal epoch.
pub fn new_epoch() -> JournalEpoch {
    JournalEpoch::from_uuid(Uuid::now_v7())
}

impl World {
    /// Lets queued work run without moving time.
    pub async fn settle(&self) {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    /// Lets the service work, advancing the clock a second per round, until `done` holds for
    /// the stored operation.
    pub async fn until(
        &self,
        op: OperationId,
        done: impl Fn(&OperationSnapshot) -> bool,
    ) -> OperationSnapshot {
        for _ in 0..4000 {
            self.settle_a_little().await;
            let snapshot = self.load(op).await;
            if done(&snapshot) {
                return snapshot;
            }
            self.advance(1);
        }
        let last = self.load(op).await;
        panic!(
            "the operation never reached the expected state; it is {:?}",
            last.operation.status
        );
    }

    /// Like [`World::until`], without moving the clock.
    pub async fn until_now(
        &self,
        op: OperationId,
        done: impl Fn(&OperationSnapshot) -> bool,
    ) -> OperationSnapshot {
        for _ in 0..400 {
            self.settle_a_little().await;
            let snapshot = self.load(op).await;
            if done(&snapshot) {
                return snapshot;
            }
        }
        let last = self.load(op).await;
        panic!(
            "the operation never reached the expected state; it is {:?}",
            last.operation.status
        );
    }

    async fn settle_a_little(&self) {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    /// Waits until the Execution step `step` has the outcome `want`.
    pub async fn until_outcome(
        &self,
        op: OperationId,
        step: domovoi_core::StepId,
        want: StepOutcome,
    ) -> OperationSnapshot {
        self.until(op, |s| {
            s.step(step).is_some_and(|r| r.progress.outcome == want)
        })
        .await
    }

    /// Waits until a Verification step of `op` has ended.
    pub async fn until_verified(&self, op: OperationId) -> OperationSnapshot {
        self.until(op, |s| {
            s.steps
                .iter()
                .any(|r| r.step.kind == StepKind::Verification && r.progress.outcome.is_final())
        })
        .await
    }

    /// Stops the running service and starts another over the same store, as a hub restart does.
    pub async fn restart(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.shutdown = tokio_util::sync::CancellationToken::new();
        let (handle, task) = self.start();
        self.handle = Some(handle);
        self.task = Some(task);
    }

    /// The skill requests the fake agent received, oldest first.
    pub fn skill_requests(&self) -> Vec<SkillDispatch> {
        self.link
            .requests()
            .into_iter()
            .filter_map(|request| match request {
                LinkRequest::Skill { dispatch, .. } => Some(dispatch),
                _ => None,
            })
            .collect()
    }

    /// Gives the agent a `restart_container` that re-checks 10 seconds after a success.
    pub fn verify_after_ten_seconds(&self) {
        let mut skill = restart_skill();
        skill.verify = Some(VerifySpec {
            recheck: true,
            delay: SignedDuration::from_secs(10),
        });
        self.link.set_skills(host_id(), vec![skill]);
    }
}
