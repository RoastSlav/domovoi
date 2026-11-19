//! The shadow recorder over a running operation service: one row per Execution step, and
//! nothing about the operation changes because of it.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::{CeilingSummary, FactId, TargetName};
use domovoi_core::policy_store::{
    DraftGuard, PolicyWrite, ShadowFilter, ShadowRecord, ShadowStatus, StoredRule,
};
use domovoi_core::ports::{
    ExecutionGuard, JournalRead, OperationWrite, Page, PolicyRepo, StateWrite, Transaction,
};
use domovoi_core::{
    Actor, ContractId, ExecutionIntent, OperationId, PlanHash, PolicyVersionId, Risk, Step, StepId,
    StepIntent, StepOutcome,
};
use domovoi_engine::policy::{
    CapabilityScope, Condition, FactState, HardLimit, HostScope, Outcome, Rule, RuleId, TargetScope,
};
use domovoi_engine::policy_shadow::{
    CeilingReading, DecisionEnvelope, LATE_AFTER, ShadowError, ShadowRecorder, decode_decision,
    decode_inputs, rules_hash, stored_rules,
};
use domovoi_testkit::build::at;

use crate::harness::{World, host_id, restart_skill, world};
use crate::shadow_env::{FakeShadowEnvironment, skill};

const SKILL_CONTRACT: &str = "skill.restart_container";

fn env() -> Arc<FakeShadowEnvironment> {
    let env = FakeShadowEnvironment::new();
    env.add_skill(&restart_skill());
    env.add_skill(&skill("inspect_logs", Risk::Read));
    Arc::new(env)
}

fn recorder(w: &World, env: Arc<FakeShadowEnvironment>) -> ShadowRecorder {
    ShadowRecorder::new(w.store.clone(), w.uow.clone(), env, w.clock.clone())
}

fn rule(id: &str, outcome: Outcome, shadow: bool) -> Rule {
    Rule {
        id: RuleId::parse(id).unwrap(),
        enabled: true,
        shadow,
        locked: false,
        host: HostScope::All,
        capability: CapabilityScope::Contract(ContractId::parse(SKILL_CONTRACT).unwrap()),
        targets: TargetScope::Any,
        conditions: Vec::new(),
        outcome,
        note: String::new(),
        version_id: None,
    }
}

/// Saves `rules` as the draft and publishes it as `number`.
async fn publish(w: &World, rules: &[Rule], number: u32) {
    publish_stored(w, stored_rules(rules), number).await;
}

/// Saves `stored` as the draft and publishes it as `number`, whatever the bodies say.
async fn publish_stored(w: &World, stored: Vec<StoredRule>, number: u32) {
    let id = PolicyVersionId::new();
    let actor = Actor::System("tests".to_owned());
    let expected = DraftGuard::Absent;
    let save = PolicyWrite::SaveDraft {
        id,
        expected,
        based_on: None,
        rules_hash: rules_hash(&stored),
        rules: stored,
        actor: actor.clone(),
        at: w.now(),
    };
    w.uow
        .commit(Transaction::new().write(StateWrite::Policy(save)))
        .await
        .unwrap();
    let publish = PolicyWrite::Publish {
        id,
        expected_revision: 1,
        number,
        acknowledgements: Vec::new(),
        actor,
        at: w.now(),
    };
    w.uow
        .commit(Transaction::new().write(StateWrite::Policy(publish)))
        .await
        .unwrap();
}

/// A new operation with one proposed restart step.
async fn step(w: &World) -> StepId {
    let op = w.create().await;
    w.propose(op).await
}

async fn rows(w: &World) -> Vec<ShadowRecord> {
    PolicyRepo::shadow_records(&*w.store, &ShadowFilter::default(), &Page::first(100))
        .await
        .unwrap()
        .items
}

/// An Execution step on a fresh operation, written directly so it can name any contract.
async fn raw_step(w: &World, contract: &str) -> OperationId {
    let op = w.create().await;
    raw_steps(w, op, contract, 1).await;
    op
}

/// `count` Execution steps appended to `op`, written directly.
async fn raw_steps(w: &World, op: OperationId, contract: &str, count: u32) {
    let intent = ExecutionIntent {
        contract: ContractId::parse(contract).unwrap(),
        contract_version: 1,
        host: host_id(),
        check: domovoi_core::CheckKey::parse("container.down").unwrap(),
        target: None,
        args: BTreeMap::new(),
        retry_of: None,
    };
    let mut tx = Transaction::new();
    for ordinal in 0..count {
        let step = Step {
            id: StepId::new(),
            operation: op,
            ordinal,
            kind: StepIntent::Execution(intent.clone()).kind(),
            phase: "investigate".to_owned(),
            parent: None,
            cycle: 0,
            intent: StepIntent::Execution(intent.clone()),
            created_at: w.now(),
        };
        let hash: PlanHash = "ab".repeat(32).parse().unwrap();
        let record = domovoi_core::ExecutionRecord::propose(
            &intent,
            step.id,
            op,
            hash,
            at(10_000),
            Risk::Write,
            w.now(),
        );
        tx = tx
            .write(StateWrite::Operation(OperationWrite::Step(step)))
            .write(StateWrite::Operation(OperationWrite::Execution {
                execution: record,
                expected: ExecutionGuard::New,
            }));
    }
    w.uow.commit(tx).await.unwrap();
}

#[tokio::test]
async fn recorder_records_one_row_per_execution_without_a_policy() {
    let w = world().await;
    for _ in 0..3 {
        step(&w).await;
    }
    let report = recorder(&w, env()).sweep_once().await.unwrap();
    assert_eq!(report.evaluated, 3);
    let rows = rows(&w).await;
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(row.status, ShadowStatus::Evaluated);
        assert_eq!(row.policy_version, 0);
        assert_eq!(row.enforced_outcome.as_deref(), Some("require_approval"));
        assert!(row.degraded, "the ceiling and autonomy are unavailable");
        let inputs = decode_inputs(row.inputs.as_deref().unwrap()).unwrap();
        assert_eq!(inputs.contract.id, SKILL_CONTRACT);
    }
}

#[tokio::test]
async fn recorder_evaluates_enforced_and_shadow_rules_separately() {
    let w = world().await;
    publish(
        &w,
        &[
            rule("enforced-deny", Outcome::Deny, false),
            rule("shadow-allow", Outcome::Allow, true),
        ],
        1,
    )
    .await;
    step(&w).await;
    recorder(&w, env()).sweep_once().await.unwrap();
    let rows = rows(&w).await;
    assert_eq!(rows[0].policy_version, 1);
    assert_eq!(rows[0].enforced_outcome.as_deref(), Some("deny"));
    // The shadow allow still meets the hard limits: no ceiling is reported, so an allow
    // is held back to an approval, and the row says so.
    assert_eq!(rows[0].shadow_outcome.as_deref(), Some("require_approval"));
    let shadow = decode_decision(rows[0].shadow.as_deref().unwrap()).unwrap();
    let DecisionEnvelope::Decision(doc) = shadow else {
        panic!("the shadow rules were evaluated");
    };
    assert!(
        doc.hard_limits_applied
            .contains(&HardLimit::CeilingDoesNotCover)
    );
    assert_eq!(doc.would_have_been.map(|d| d.outcome), Some(Outcome::Allow));
    let filter = ShadowFilter {
        differing: true,
        ..ShadowFilter::default()
    };
    let differing = PolicyRepo::shadow_records(&*w.store, &filter, &Page::first(10))
        .await
        .unwrap();
    assert_eq!(differing.items.len(), 1);
}

#[tokio::test]
async fn recorder_is_idempotent_across_sweeps_and_restarts() {
    let w = world().await;
    step(&w).await;
    step(&w).await;
    let first = recorder(&w, env());
    first.sweep_once().await.unwrap();
    let again = first.sweep_once().await.unwrap();
    assert_eq!(again.batches, 0, "nothing left to record");
    let restarted = recorder(&w, env()).sweep_once().await.unwrap();
    assert_eq!(
        restarted.evaluated + restarted.unevaluable + restarted.skipped,
        0
    );
    assert_eq!(rows(&w).await.len(), 2);
    let state = PolicyRepo::shadow_state(&*w.store).await.unwrap();
    assert_eq!(state.total_evaluated, 2);
}

#[tokio::test]
async fn recorder_skips_packages_and_marks_unevaluable_steps() {
    let w = world().await;
    raw_step(&w, "agent.package.install").await;
    step(&w).await;
    // This environment never heard of the skill, so the ordinary step cannot be evaluated.
    let report = recorder(&w, Arc::new(FakeShadowEnvironment::new()))
        .sweep_once()
        .await
        .unwrap();
    assert_eq!(
        (report.skipped, report.unevaluable, report.evaluated),
        (1, 1, 0)
    );
    let rows = rows(&w).await;
    let skipped = rows
        .iter()
        .find(|r| r.status == ShadowStatus::Skipped)
        .unwrap();
    assert_eq!(skipped.reason.as_deref(), Some("package_contract"));
    assert!(skipped.inputs.is_none() && skipped.enforced.is_none());
    let unevaluable = rows
        .iter()
        .find(|r| r.status == ShadowStatus::Unevaluable)
        .unwrap();
    assert_eq!(unevaluable.reason.as_deref(), Some("contract_unknown"));
    let next = recorder(&w, env()).sweep_once().await.unwrap();
    assert_eq!(next.batches, 0, "neither row comes back");
}

#[tokio::test]
async fn recorder_marks_late_rows() {
    let w = world().await;
    step(&w).await;
    w.advance(LATE_AFTER.as_secs() * 2);
    recorder(&w, env()).sweep_once().await.unwrap();
    let row = rows(&w).await.remove(0);
    assert_eq!(row.status, ShadowStatus::Evaluated);
    assert_eq!(row.reason.as_deref(), Some("late"));
}

#[tokio::test]
async fn recorder_writes_no_journal_and_no_audit() {
    let w = world().await;
    step(&w).await;
    let head = JournalRead::head_seq(&*w.store).await.unwrap();
    let audit = w.audit_kinds().await.len();
    recorder(&w, env()).sweep_once().await.unwrap();
    assert_eq!(rows(&w).await.len(), 1);
    assert_eq!(JournalRead::head_seq(&*w.store).await.unwrap(), head);
    assert_eq!(w.audit_kinds().await.len(), audit);
}

#[tokio::test]
async fn recorder_fails_closed_on_unreadable_rules() {
    let w = world().await;
    publish(&w, &[rule("r", Outcome::Deny, false)], 1).await;
    step(&w).await;
    // Version 2 holds a body with an envelope version no build knows.
    let mut bad = stored_rules(&[rule("r", Outcome::Deny, false)]);
    bad[0].body = bad[0].body.replace("\"v\":1", "\"v\":9");
    publish_stored(&w, bad, 2).await;
    let broken = recorder(&w, env());
    assert!(matches!(
        broken.sweep_once().await,
        Err(ShadowError::Rules(_))
    ));
    assert!(rows(&w).await.is_empty());
    let state = PolicyRepo::shadow_state(&*w.store).await.unwrap();
    assert_eq!(state.last_error.as_deref(), Some("stored rules unreadable"));

    publish(&w, &[rule("r", Outcome::Deny, false)], 3).await;
    broken.sweep_once().await.unwrap();
    assert_eq!(rows(&w).await.len(), 1);
    let state = PolicyRepo::shadow_state(&*w.store).await.unwrap();
    assert!(state.last_error.is_none());
}

#[tokio::test]
async fn recorder_drains_a_backlog_in_bounded_batches() {
    let w = world().await;
    let op = w.create().await;
    raw_steps(&w, op, "agent.package.install", 120).await;
    let recorder = recorder(&w, env());
    let report = recorder.sweep_once().await.unwrap();
    assert_eq!((report.skipped, report.batches), (120, 3));
    let more = w.create().await;
    raw_steps(&w, more, "agent.package.list", 600).await;
    let big = recorder.sweep_once().await.unwrap();
    assert_eq!((big.skipped, big.batches), (500, 10));
    let rest = recorder.sweep_once().await.unwrap();
    assert_eq!(rest.skipped, 100);
}

#[tokio::test]
async fn shadow_mode_does_not_change_the_operation_outcome() {
    async fn run(record: bool) -> Vec<(String, StepOutcome)> {
        let w = world().await;
        let recorder = recorder(&w, env());
        let op = w.create().await;
        if record {
            recorder.sweep_once().await.unwrap();
        }
        let step = w.propose(op).await;
        if record {
            recorder.sweep_once().await.unwrap();
        }
        w.approve(op, step).await.unwrap();
        if record {
            recorder.sweep_once().await.unwrap();
        }
        let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;
        let mut trace: Vec<(String, StepOutcome)> = snapshot
            .steps
            .iter()
            .map(|s| (format!("{:?}", s.step.kind), s.progress.outcome))
            .collect();
        trace.push((
            format!("{:?}", snapshot.operation.status),
            StepOutcome::Succeeded,
        ));
        trace
    }
    let without = run(false).await;
    let with = run(true).await;
    assert_eq!(with, without);
}

#[tokio::test]
async fn running_recorder_task_stops_on_cancel() {
    let w = world().await;
    step(&w).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let task = tokio::spawn(recorder(&w, env()).run(cancel.clone()));
    // The fake clock never moves, so the loop records once and then waits on it.
    for _ in 0..200 {
        if !rows(&w).await.is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    cancel.cancel();
    task.await.unwrap();
    assert_eq!(rows(&w).await.len(), 1);
}

#[tokio::test]
async fn recorder_records_a_reported_ceiling_without_a_ceiling_gap() {
    let w = world().await;
    step(&w).await;
    let env = env();
    env.set_ceiling(
        host_id(),
        CeilingReading::Reported(CeilingSummary {
            enabled: true,
            entries: Vec::new(),
            hash: Some("h1".to_owned()),
        }),
    );
    recorder(&w, env).sweep_once().await.unwrap();
    let row = rows(&w).await.remove(0);
    assert!(!row.gaps.contains("ceiling"), "{}", row.gaps);
    let inputs = decode_inputs(row.inputs.as_deref().unwrap()).unwrap();
    assert_eq!(inputs.ceiling.unwrap().hash.as_deref(), Some("h1"));
}

#[tokio::test]
async fn recorder_marks_an_unresolved_host_unevaluable() {
    let w = world().await;
    step(&w).await;
    let env = env();
    env.fail_host(host_id());
    recorder(&w, env).sweep_once().await.unwrap();
    let row = rows(&w).await.remove(0);
    assert_eq!(row.status, ShadowStatus::Unevaluable);
    assert_eq!(row.reason.as_deref(), Some("target_unresolved"));
}

#[tokio::test]
async fn recorder_reads_an_unknown_root_state_as_root() {
    let w = world().await;
    step(&w).await;
    let env = env();
    env.set_runs_as_root(host_id(), None);
    recorder(&w, env).sweep_once().await.unwrap();
    let row = rows(&w).await.remove(0);
    let inputs = decode_inputs(row.inputs.as_deref().unwrap()).unwrap();
    assert!(inputs.runs_as_root);
    assert!(row.gaps.contains("runs_as_root"));
    assert_eq!(row.enforced_outcome.as_deref(), Some("require_approval"));
}

#[tokio::test]
async fn recorder_records_rule_targets_and_known_facts() {
    let w = world().await;
    let named = Rule {
        targets: TargetScope::Exact([TargetName::parse("nas").unwrap()].into()),
        conditions: vec![Condition::RequiresFact {
            fact: FactId::parse("host.not_in_maintenance").unwrap(),
            expected: true,
        }],
        ..rule("named", Outcome::Allow, false)
    };
    publish(&w, &[named], 1).await;
    step(&w).await;
    let env = env();
    env.set_host_name(host_id(), "nas");
    env.set_fact(host_id(), "host.not_in_maintenance", FactState::Known(true));
    recorder(&w, env).sweep_once().await.unwrap();
    let row = rows(&w).await.remove(0);
    let inputs = decode_inputs(row.inputs.as_deref().unwrap()).unwrap();
    assert!(inputs.rule_targets.contains_key("nas"));
    assert!(inputs.facts.contains_key("host.not_in_maintenance"));
    assert!(!row.gaps.contains("facts"), "{}", row.gaps);
    let DecisionEnvelope::Decision(doc) =
        decode_decision(row.enforced.as_deref().unwrap()).unwrap()
    else {
        panic!("evaluated");
    };
    assert_eq!(doc.winners.len(), 1);
}
