//! A seeded simulator over the real operation service. One operation, sixty actions drawn
//! from a small generator: evidence that works, fails or times out; runs that succeed, fail,
//! hang or leave an unknown outcome; approvals, denials, questions and answers; time moving
//! on; the agent coming and going; service restarts; and the rare cancel, finish, pause and
//! resume. After every action the stored operation is read back and nine invariants checked.
//! The seed is in every failure message.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use domovoi_core::capability::{Availability, ContractId};
use domovoi_core::ports::{
    ActionAnswer, AggregateRef, EffectStatus, JournalRead, LinkError, Page, Presence, ToolOutput,
    UnknownReason,
};
use domovoi_core::{
    Actor, BudgetKind, EdgeKind, ExecutionId, FinishOutcome, OperationId, OperationSnapshot,
    OperationStatus, PauseReason, StepId, StepIntent, StepKind, StepOutcome, build_trace,
};
use domovoi_engine::operations::{NewStep, derive_status};
use domovoi_testkit::approvals;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use crate::drive::{failed_output, firing, ok_output, resolved};
use crate::harness::{World, host_id, world};

/// Text every skill and tool output carries, so a leak into the journal or audit log shows.
const MARKER: &str = "zq-marker-5531";

/// A small linear congruential generator, so a seed always draws the same actions.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[usize::try_from(self.below(items.len() as u64)).unwrap_or(0)]
    }
}

/// What the fake agent does with one run.
#[derive(Clone, Copy)]
enum RunMode {
    Works,
    Fails,
    Hangs,
}

/// What the fake agent does with one read.
#[derive(Clone, Copy)]
enum ReadMode {
    Works,
    Transport,
    Timeout,
}

/// What the agent's scripted behaviour is right now.
#[derive(Default)]
struct Script {
    reads: VecDeque<ReadMode>,
    runs: BTreeMap<ExecutionId, RunMode>,
    checks: VecDeque<bool>,
}

/// One simulated run of one operation.
struct Sim {
    w: World,
    op: OperationId,
    rng: Lcg,
    script: Arc<Mutex<Script>>,
    restarts: bool,
    seed: u64,
    /// What this run has been through, as `kind:outcome` and `status:name` labels.
    seen: std::collections::BTreeSet<String>,
}

/// Locks the script, whatever happened to its last holder.
fn locked(script: &Mutex<Script>) -> std::sync::MutexGuard<'_, Script> {
    script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Sim {
    async fn new(seed: u64, restarts: bool) -> Self {
        let w = world().await;
        w.verify_after_ten_seconds();
        let script = Arc::new(Mutex::new(Script::default()));
        let reads = script.clone();
        w.link.on_tool(move |_, _| {
            let mode = locked(&reads).reads.pop_front().unwrap_or(ReadMode::Works);
            match mode {
                ReadMode::Works => Ok(ToolOutput {
                    // Longer than a summary, so the marker stays out of the one-line summary.
                    text: format!("{}{MARKER}", "overview line ".repeat(30)),
                    truncated: false,
                }),
                ReadMode::Transport => Err(LinkError::Transport("reset".to_owned())),
                ReadMode::Timeout => Err(LinkError::Timeout),
            }
        });
        let runs = script.clone();
        w.link.on_skill(move |_, dispatch| {
            let mode = locked(&runs).runs.get(&dispatch.execution).copied();
            match mode.unwrap_or(RunMode::Works) {
                RunMode::Works => Ok(ok_output(MARKER)),
                RunMode::Fails => Ok(failed_output(1)),
                RunMode::Hangs => Err(LinkError::Indeterminate("link dropped".to_owned())),
            }
        });
        let checks = script.clone();
        w.link.on_check(move |_, _, _| {
            let passes = locked(&checks).checks.pop_front().unwrap_or(true);
            Ok(if passes { resolved() } else { firing() })
        });
        let op = w.create().await;
        Self {
            w,
            op,
            rng: Lcg(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1)),
            script,
            restarts,
            seed,
            seen: std::collections::BTreeSet::new(),
        }
    }

    /// Runs one random action and returns its name.
    async fn act(&mut self) -> &'static str {
        self.w.advance(1);
        let roll = self.rng.below(90);
        match roll {
            0..=11 => self.evidence().await,
            12..=25 => self.execute().await,
            26..=39 => self.approve().await,
            40..=42 => self.deny().await,
            43..=47 => self.ask().await,
            48..=52 => self.answer().await,
            53..=70 => self.pass_time().await,
            71..=75 => self.toggle_agent(),
            76..=78 => self.change_availability(),
            79..=82 => self.restart().await,
            83 => self.cancel().await,
            84 => self.finish().await,
            85..=86 => self.pause().await,
            _ => self.resume().await,
        }
    }

    async fn evidence(&mut self) -> &'static str {
        let modes: &[&[ReadMode]] = &[
            &[ReadMode::Works],
            &[ReadMode::Transport],
            &[
                ReadMode::Transport,
                ReadMode::Transport,
                ReadMode::Transport,
            ],
            &[ReadMode::Timeout],
        ];
        locked(&self.script)
            .reads
            .extend(self.rng.pick(modes).iter().copied());
        let step = NewStep {
            id: StepId::new(),
            kind: StepKind::EvidenceRequest,
            phase: "evidence".to_owned(),
            parent: None,
            intent: StepIntent::EvidenceRequest {
                host: host_id(),
                tool: "host_overview".to_owned(),
                args: serde_json::json!({}),
            },
        };
        let _ = self
            .w
            .handle()
            .propose(self.op, vec![step], Vec::new())
            .await;
        "evidence"
    }

    async fn execute(&mut self) -> &'static str {
        let container = *self.rng.pick(&["plex", "emby", "jellyfin"]);
        let mode = *self.rng.pick(&[
            RunMode::Works,
            RunMode::Works,
            RunMode::Fails,
            RunMode::Hangs,
        ]);
        let Ok(step) = self
            .w
            .handle()
            .propose_execution(self.op, crate::harness::restart(container))
            .await
        else {
            return "execute (refused)";
        };
        let action = ExecutionId::from_step(step);
        locked(&self.script).runs.insert(action, mode);
        if matches!(mode, RunMode::Hangs) {
            // What the agent's journal will say when asked later.
            let answer = *self.rng.pick(&[Some(true), Some(false), None]);
            match answer {
                Some(succeeded) => self.w.link.answer(
                    host_id(),
                    action,
                    ActionAnswer::Finished {
                        succeeded,
                        detail: None,
                    },
                ),
                None => self.w.link.answer(
                    host_id(),
                    action,
                    ActionAnswer::Unknown {
                        reason: UnknownReason::AgentRestarted,
                    },
                ),
            }
        }
        "execute"
    }

    /// Pending Execution steps that still wait for a person.
    fn awaiting(snapshot: &OperationSnapshot) -> Vec<StepId> {
        snapshot
            .steps
            .iter()
            .filter(|record| {
                record.step.kind == StepKind::Execution
                    && record.progress.outcome == StepOutcome::Pending
                    && snapshot
                        .execution_of(record.step.id)
                        .is_some_and(|row| row.value.approval().is_none())
            })
            .map(|record| record.step.id)
            .collect()
    }

    async fn approve(&mut self) -> &'static str {
        let snapshot = self.w.load(self.op).await;
        let waiting = Self::awaiting(&snapshot);
        if waiting.is_empty() {
            return "approve (nothing waits)";
        }
        let step = *self.rng.pick(&waiting);
        let _ = self.w.approve(self.op, step).await;
        "approve"
    }

    async fn deny(&mut self) -> &'static str {
        let snapshot = self.w.load(self.op).await;
        let waiting = Self::awaiting(&snapshot);
        if waiting.is_empty() {
            return "deny (nothing waits)";
        }
        let step = *self.rng.pick(&waiting);
        let human = approvals::human(self.w.user);
        let _ = self.w.handle().deny(self.op, step, human).await;
        "deny"
    }

    async fn ask(&mut self) -> &'static str {
        let step = NewStep {
            id: StepId::new(),
            kind: StepKind::Question,
            phase: "ask".to_owned(),
            parent: None,
            intent: StepIntent::Question {
                text: "Restart it?".to_owned(),
                expires_at: self.w.now() + SignedDuration::from_hours(1),
            },
        };
        let _ = self
            .w
            .handle()
            .propose(self.op, vec![step], Vec::new())
            .await;
        "ask"
    }

    async fn answer(&mut self) -> &'static str {
        let snapshot = self.w.load(self.op).await;
        let open: Vec<StepId> = snapshot
            .steps
            .iter()
            .filter(|r| {
                r.step.kind == StepKind::Question && r.progress.outcome == StepOutcome::Pending
            })
            .map(|r| r.step.id)
            .collect();
        if open.is_empty() {
            return "answer (nothing asked)";
        }
        let question = *self.rng.pick(&open);
        let actor = Actor::User(self.w.user);
        let _ = self
            .w
            .handle()
            .answer(self.op, question, "yes".to_owned(), actor)
            .await;
        "answer"
    }

    async fn pass_time(&mut self) -> &'static str {
        let seconds = *self.rng.pick(&[2, 11, 31, 61, 61, 301, 3601, 90_000]);
        self.w.advance(seconds);
        if self.rng.below(3) == 0 {
            locked(&self.script)
                .checks
                .push_back(self.rng.below(2) == 0);
        }
        "pass time"
    }

    fn toggle_agent(&mut self) -> &'static str {
        let connected = matches!(
            domovoi_core::ports::AgentLink::presence(&*self.w.link, host_id()),
            Presence::Connected { .. }
        );
        let presence = if connected {
            Presence::Disconnected {
                last_seen: Some(T0),
            }
        } else {
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            }
        };
        self.w.link.set_presence(host_id(), presence);
        "toggle the agent"
    }

    fn change_availability(&mut self) -> &'static str {
        let states = [
            Availability::Available,
            Availability::Available,
            Availability::HostAbsent { since: T0 },
            Availability::HostIncompatible {
                reported: "9.0".to_owned(),
            },
            Availability::DisabledByHub,
        ];
        let state = self.rng.pick(&states);
        let contract = ContractId::parse("skill.restart_container").unwrap();
        self.w
            .registry
            .set_availability(host_id(), &contract, state.clone());
        "change availability"
    }

    async fn restart(&mut self) -> &'static str {
        if self.restarts {
            self.w.restart().await;
        }
        "restart"
    }

    async fn cancel(&mut self) -> &'static str {
        let actor = Actor::User(self.w.user);
        let _ = self
            .w
            .handle()
            .cancel(self.op, actor, "stop".to_owned())
            .await;
        "cancel"
    }

    async fn finish(&mut self) -> &'static str {
        let _ = self
            .w
            .handle()
            .finish(self.op, FinishOutcome::Succeeded, "done".to_owned())
            .await;
        "finish"
    }

    async fn pause(&mut self) -> &'static str {
        let actor = Actor::User(self.w.user);
        let _ = self.w.handle().pause(self.op, actor).await;
        "pause"
    }

    async fn resume(&mut self) -> &'static str {
        let actor = Actor::User(self.w.user);
        let extend = vec![
            (BudgetKind::Executions, Some(10)),
            (BudgetKind::EvidenceRequests, Some(60)),
            (BudgetKind::Cycles, Some(6)),
        ];
        let _ = self.w.handle().resume(self.op, actor, extend).await;
        "resume"
    }

    /// Lets the runner and the writer finish what they are doing, without moving time.
    async fn quiesce(&self) {
        let mut last = None;
        let mut stable = 0;
        for _ in 0..80 {
            for _ in 0..15 {
                tokio::task::yield_now().await;
            }
            let snapshot = self.w.load(self.op).await;
            let print = (
                snapshot.version,
                snapshot.steps.len(),
                self.w.effects(self.op, EffectStatus::Pending).await.len(),
                self.w.effects(self.op, EffectStatus::Claimed).await.len(),
                self.w.skill_requests().len(),
            );
            if last == Some(print) {
                stable += 1;
                if stable >= 2 {
                    return;
                }
            } else {
                stable = 0;
                last = Some(print);
            }
        }
    }

    /// The nine invariants, read from what is stored.
    async fn check(&mut self, number: usize, action: &str) {
        let at = format!("seed {} action {number} ({action})", self.seed);
        let snapshot = self.w.load(self.op).await;
        self.seen
            .insert(format!("status:{}", snapshot.operation.status.name()));
        for record in &snapshot.steps {
            self.seen.insert(format!(
                "{}:{}",
                record.step.kind.as_str(),
                record.progress.outcome.as_str()
            ));
        }
        let journal = JournalRead::entries_for(
            &*self.w.store,
            &AggregateRef::operation(self.op),
            Page::MAX_LIMIT,
        )
        .await
        .unwrap();
        self.i1_to_i2(&snapshot, &at);
        self.i3(&snapshot, &at);
        i4_to_i5(&snapshot, &at);
        i6(&snapshot, &journal, &at);
        i7(&snapshot, &at);
        self.i8(&snapshot, &at).await;
        self.i9(&journal, &at).await;
        // The trace always builds, and agrees with the stored steps.
        let trace = build_trace(&snapshot, journal.last().map_or(0, |e| e.seq));
        assert_eq!(trace.steps.len(), snapshot.steps.len(), "{at}: trace steps");
    }

    /// I1: everything belongs to the operation and ordinals have no gap. I2: edges and
    /// parents point at earlier steps and obey the endpoint rules.
    fn i1_to_i2(&self, snapshot: &OperationSnapshot, at: &str) {
        for (index, record) in snapshot.steps.iter().enumerate() {
            assert_eq!(record.step.operation, self.op, "{at}: I1 step operation");
            assert_eq!(record.step.ordinal as usize, index, "{at}: I1 ordinals");
            if let Some(parent) = record.step.parent {
                let parent = snapshot
                    .step(parent)
                    .unwrap_or_else(|| panic!("{at}: I2 parent"));
                assert!(
                    parent.step.ordinal < record.step.ordinal,
                    "{at}: I2 parent order"
                );
            }
        }
        for edge in &snapshot.edges {
            assert_eq!(edge.operation, self.op, "{at}: I1 edge operation");
            let (Some(from), Some(to)) = (snapshot.step(edge.from), snapshot.step(edge.to)) else {
                panic!("{at}: I2 edge endpoint is missing");
            };
            assert!(
                to.step.ordinal < from.step.ordinal,
                "{at}: I2 edge direction"
            );
            let (want_from, want_to) = match edge.kind {
                EdgeKind::Verifies => (Some(StepKind::Verification), Some(StepKind::Execution)),
                EdgeKind::RetryOf => (Some(StepKind::Execution), Some(StepKind::Execution)),
                EdgeKind::Answers => (Some(StepKind::Note), Some(StepKind::Question)),
                EdgeKind::DependsOn | EdgeKind::CausedBy => (None, None),
            };
            assert!(
                want_from.is_none_or(|kind| kind == from.step.kind)
                    && want_to.is_none_or(|kind| kind == to.step.kind),
                "{at}: I2 edge kinds"
            );
        }
        for row in &snapshot.executions {
            assert_eq!(row.value.operation, self.op, "{at}: I1 execution operation");
        }
    }

    /// I3: each Execution step has exactly one row, and no run reached the agent twice.
    fn i3(&self, snapshot: &OperationSnapshot, at: &str) {
        for record in &snapshot.steps {
            let rows = snapshot
                .executions
                .iter()
                .filter(|row| row.value.step == record.step.id)
                .count();
            let expected = usize::from(
                record.step.kind == StepKind::Execution && has_row(snapshot, record.step.id),
            );
            assert_eq!(rows, expected, "{at}: I3 one row per execution step");
        }
        for row in &snapshot.executions {
            assert_eq!(
                row.value.execution,
                ExecutionId::from_step(row.value.step),
                "{at}: I3 id"
            );
        }
        let mut seen = BTreeMap::new();
        for request in self.w.skill_requests() {
            *seen.entry(request.execution).or_insert(0) += 1;
        }
        assert!(
            seen.values().all(|count| *count <= 1),
            "{at}: I3 a run was sent twice"
        );
    }

    /// I8: no effect of a finished operation is still claimed once the runner drained.
    async fn i8(&self, snapshot: &OperationSnapshot, at: &str) {
        if snapshot.operation.status.is_terminal() {
            let claimed = self.w.effects(self.op, EffectStatus::Claimed).await;
            assert!(
                claimed.is_empty(),
                "{at}: I8 claimed effects of a finished operation"
            );
        }
    }

    /// I9: no output text reached the journal or the audit log.
    async fn i9(&self, journal: &[domovoi_core::ports::JournalEntry], at: &str) {
        for entry in journal {
            assert!(
                !entry.payload.to_string().contains(MARKER),
                "{at}: I9 journal"
            );
        }
        let page = domovoi_core::ports::AuditLog::list(
            &*self.w.store,
            &domovoi_core::ports::AuditFilter::default(),
            &Page::first(Page::MAX_LIMIT),
        )
        .await
        .unwrap();
        for event in page.items {
            assert!(!event.detail.to_string().contains(MARKER), "{at}: I9 audit");
        }
    }

    /// The operation as a trace with ids replaced by what they are, so two runs compare.
    async fn normalised(&self) -> String {
        let snapshot = self.w.load(self.op).await;
        let mut text = serde_json::to_string(&build_trace(&snapshot, 0)).unwrap();
        for record in &snapshot.steps {
            text = text.replace(
                &record.step.id.to_string(),
                &format!("step-{}", record.step.ordinal),
            );
        }
        text.replace(&self.op.to_string(), "operation")
    }
}

/// Whether the Execution step has a stored row: a refused one never gets one.
fn has_row(snapshot: &OperationSnapshot, step: StepId) -> bool {
    snapshot.execution_of(step).is_some()
        || snapshot
            .edges
            .iter()
            .any(|edge| edge.from == step && edge.kind == EdgeKind::DependsOn)
}

/// I4: the stored status is the derived one while the operation is open and running. I5: a
/// finished operation has no pending step, and only a dispatched execution still runs.
fn i4_to_i5(snapshot: &OperationSnapshot, at: &str) {
    let status = &snapshot.operation.status;
    if !matches!(
        status,
        OperationStatus::Paused { .. } | OperationStatus::Queued
    ) && !status.is_terminal()
    {
        assert_eq!(
            *status,
            derive_status(snapshot),
            "{at}: I4 status is derived"
        );
    }
    if status.is_terminal() {
        for record in &snapshot.steps {
            assert_ne!(
                record.progress.outcome,
                StepOutcome::Pending,
                "{at}: I5 pending step"
            );
            if record.progress.outcome == StepOutcome::Running {
                let sent = snapshot
                    .execution_of(record.step.id)
                    .is_some_and(|row| row.value.dispatched.is_some());
                assert!(
                    record.step.kind == StepKind::Execution && sent,
                    "{at}: I5 only a dispatched execution may still run"
                );
            }
        }
    }
}

/// I6: the journal's sequence only grows, and every step that left pending says so there.
fn i6(snapshot: &OperationSnapshot, journal: &[domovoi_core::ports::JournalEntry], at: &str) {
    assert!(
        journal.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "{at}: I6 order"
    );
    for record in &snapshot.steps {
        let born_done = matches!(
            record.step.kind,
            StepKind::Assessment | StepKind::Plan | StepKind::Note
        );
        if record.progress.outcome == StepOutcome::Pending || born_done {
            continue;
        }
        let id = record.step.id.to_string();
        let said = journal.iter().any(|entry| {
            entry.payload.get("step").and_then(|s| s.as_str()) == Some(id.as_str())
                && matches!(
                    entry.kind.as_str(),
                    "operation.step_started" | "operation.step_finished"
                )
        });
        assert!(
            said,
            "{at}: I6 the {:?} step has no journal entry",
            record.step.kind
        );
    }
}

/// I7: counted budgets stay within their limits, and a budget pause means the limit is spent.
fn i7(snapshot: &OperationSnapshot, at: &str) {
    let budgets = &snapshot.operation.budgets;
    let (limits, used) = (&budgets.limits, &budgets.used);
    for (limit, used, what) in [
        (limits.executions, used.executions, "executions"),
        (limits.evidence_requests, used.evidence_requests, "evidence"),
        (limits.model_turns, used.model_turns, "model turns"),
    ] {
        assert!(
            limit.is_none_or(|limit| used <= limit),
            "{at}: I7 {what} past its limit"
        );
    }
    if let OperationStatus::Paused {
        reason: PauseReason::Budget { exhausted },
    } = &snapshot.operation.status
    {
        assert!(
            budgets.is_spent(*exhausted, snapshot.operation.cycle),
            "{at}: I7 paused for a budget that still has room"
        );
    }
}

/// Runs one seed to the end and returns the normalised trace and what the run went through.
async fn run(seed: u64, restarts: bool) -> (String, std::collections::BTreeSet<String>) {
    let mut sim = Sim::new(seed, restarts).await;
    sim.quiesce().await;
    for number in 0..60 {
        let action = sim.act().await;
        sim.quiesce().await;
        sim.check(number, action).await;
    }
    (sim.normalised().await, sim.seen)
}

#[tokio::test]
async fn simulator_keeps_the_invariants_for_seeds_0_to_199() {
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..200 {
        seen.extend(run(seed, true).await.1);
    }
    // The generator reaches the interesting places, so the invariants were checked there.
    for label in [
        "execution:succeeded",
        "execution:failed",
        "execution:refused",
        "execution:outcome_unknown",
        "execution:unavailable",
        "execution:not_run",
        "verification:succeeded",
        "verification:failed",
        "evidence_request:succeeded",
        "evidence_request:unavailable",
        "evidence_request:failed",
        "question:succeeded",
        "approval:refused",
        "status:waiting_approval",
        "status:waiting_agent",
        "status:paused",
        "status:succeeded",
        "status:cancelled",
    ] {
        assert!(
            seen.contains(label),
            "no seed reached {label}; saw {seen:?}"
        );
    }
}

#[tokio::test]
async fn simulator_restart_equivalence() {
    for seed in 0..20 {
        let with = run(seed, true).await.0;
        let without = run(seed, false).await.0;
        assert_eq!(with, without, "seed {seed}: a restart changed the outcome");
    }
}

#[tokio::test]
async fn simulator_replays_the_same_trace_for_the_same_seed() {
    for seed in [3, 41, 97, 150] {
        let first = run(seed, true).await.0;
        let second = run(seed, true).await.0;
        assert_eq!(
            first, second,
            "seed {seed}: the same seed gave another trace"
        );
    }
}
