//! The dispatch boundary and what follows a run: the skill is sent under the step's id, the
//! checks before it, the loop guard, and the verification after it.

use domovoi_core::capability::{Availability, ContractId};
use domovoi_core::ports::{
    AggregateRef, EffectDraft, EffectKind, EffectStatus, Presence, Transaction,
};
use domovoi_core::{
    EffectId, ExecutionId, KeyFingerprint, OperationStatus, PlanHash, SkillName, StepKind,
    StepOutcome,
};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use serde_json::json;

use crate::drive::{failed_output, firing};
use crate::harness::{World, host_id, restart_skill, world, world_gated, world_with};

/// The host drops off the link, as a lost connection does.
fn disconnect(w: &World) {
    w.link.set_presence(
        host_id(),
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
}

/// The host is back on the link.
fn reconnect(w: &World) {
    w.link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
}

/// The reason stored on the Execution step.
async fn reason(w: &World, op: domovoi_core::OperationId, step: domovoi_core::StepId) -> String {
    w.load(op)
        .await
        .step(step)
        .unwrap()
        .progress
        .reason
        .clone()
        .unwrap_or_default()
}

#[tokio::test]
async fn dispatch_runs_the_skill_under_the_step_id() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    let plan = w.load(op).await.execution_of(step).unwrap().value.plan_hash;
    w.approve(op, step).await.unwrap();

    let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let sent = w.skill_requests();
    assert_eq!(sent.len(), 1, "the agent got exactly one request");
    assert_eq!(sent[0].action, ExecutionId::from_step(step).as_action());
    assert_eq!(sent[0].skill.as_str(), "restart_container");
    let skill = restart_skill();
    let wire = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &sent[0].args);
    assert_eq!(sent[0].plan_hash, wire, "the wire carries the v1 hash");
    assert_ne!(sent[0].plan_hash, plan, "the approved v2 hash stays on hub");
    let row = &snapshot.execution_of(step).unwrap().value;
    assert!(row.dispatched.is_some() && row.settled_at.is_some());
    let audit = w.audit_kinds().await;
    assert!(audit.contains(&"execution.dispatched".to_owned()));
    assert!(audit.contains(&"execution.succeeded".to_owned()));
    let output = snapshot.step(step).unwrap().progress.output.clone();
    assert_eq!(output.as_deref(), Some("restarted plex"));
    assert!(
        w.effects(op, EffectStatus::Pending)
            .await
            .iter()
            .all(|effect| effect.kind != "dispatch"),
        "the dispatch effect was completed by the dispatch commit"
    );
}

#[tokio::test]
async fn dispatch_is_idempotent_by_step_id() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    // A second delivery of the same dispatch, as a duplicate in the outbox would be.
    let now = w.now();
    let again = EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Dispatch,
        aggregate: AggregateRef::operation(op),
        payload: json!({ "v": 1, "operation": op.to_string(), "step": step.to_string() }),
        not_before: now,
        max_attempts: 5,
        dedup_key: None,
        created_at: now,
    };
    w.uow
        .commit(Transaction::new().effect(again))
        .await
        .unwrap();
    for _ in 0..40 {
        w.settle().await;
        w.advance(1);
    }

    assert_eq!(w.skill_requests().len(), 1, "the run was not sent twice");
    for status in [EffectStatus::Pending, EffectStatus::Claimed] {
        assert!(
            w.effects(op, status)
                .await
                .iter()
                .all(|effect| effect.kind != "dispatch"),
            "the duplicate was completed without sending"
        );
    }
}

#[tokio::test]
async fn dispatch_refused_when_the_plan_changed() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    // The agent now has another entry executable under the same skill name.
    let mut changed = restart_skill();
    changed.entry_script_digest = domovoi_core::ScriptDigest::of_bytes(b"another entry");
    w.link.set_skills(host_id(), vec![changed]);
    w.queue.open();

    let snapshot = w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(
        snapshot.step(step).unwrap().progress.reason.as_deref(),
        Some("the plan changed after approval")
    );
    assert!(w.skill_requests().is_empty(), "nothing reached the agent");
}

#[tokio::test]
async fn dispatch_refused_when_the_host_was_re_enrolled() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    // The host enrolled again under another key since the plan was shown.
    w.resolver
        .set_key(host_id(), KeyFingerprint::of_public_key(&[9u8; 32]));
    w.queue.open();

    let snapshot = w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(
        snapshot.step(step).unwrap().progress.reason.as_deref(),
        Some("the plan changed after approval")
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn dispatch_waits_for_an_absent_agent_then_runs() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    disconnect(&w);
    w.approve(op, step).await.unwrap();

    let waiting = w
        .until_now(op, |s| s.operation.status == OperationStatus::WaitingAgent)
        .await;
    assert_eq!(
        waiting.step(step).unwrap().progress.outcome,
        StepOutcome::Pending
    );
    assert!(
        waiting
            .execution_of(step)
            .unwrap()
            .value
            .waiting_since
            .is_some()
    );
    assert!(w.skill_requests().is_empty());

    reconnect(&w);
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    assert_eq!(w.skill_requests().len(), 1);
}

#[tokio::test]
async fn dispatch_becomes_unavailable_after_the_wait_limit() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    disconnect(&w);
    w.approve(op, step).await.unwrap();
    w.until_now(op, |s| s.operation.status == OperationStatus::WaitingAgent)
        .await;

    w.advance(31 * 60);
    w.until_outcome(op, step, StepOutcome::Unavailable).await;

    assert_eq!(
        reason(&w, op, step).await,
        "the agent did not come back in time"
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn dispatch_to_an_incompatible_host_is_unavailable_at_once() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.registry.set_availability(
        host_id(),
        &ContractId::parse("skill.restart_container").unwrap(),
        Availability::HostIncompatible {
            reported: "9.0".to_owned(),
        },
    );
    w.approve(op, step).await.unwrap();

    let snapshot = w.until_outcome(op, step, StepOutcome::Unavailable).await;

    assert_eq!(reason(&w, op, step).await, "agent upgrade required");
    assert!(
        snapshot
            .execution_of(step)
            .unwrap()
            .value
            .waiting_since
            .is_none()
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn dispatch_refused_when_the_registry_says_disabled() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.registry
        .disable(host_id(), SkillName::parse("restart_container").unwrap());
    w.approve(op, step).await.unwrap();

    w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(
        reason(&w, op, step).await,
        domovoi_core::ports::DISABLED_REASON
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn loop_guard_blocks_the_fourth_run_in_an_hour() {
    let w = world().await;
    let op = w.create().await;
    for _ in 0..3 {
        let step = w.propose(op).await;
        w.approve(op, step).await.unwrap();
        w.until_outcome(op, step, StepOutcome::Succeeded).await;
    }
    let fourth = w.propose(op).await;
    w.approve(op, fourth).await.unwrap();

    w.until_outcome(op, fourth, StepOutcome::Refused).await;

    assert!(reason(&w, op, fourth).await.starts_with("loop guard:"));
    assert_eq!(w.skill_requests().len(), 3, "the fourth was never sent");
    let blocked = w.audit_event("execution.blocked").await;
    assert!(blocked.is_some(), "the block is audited as blocked");
    assert!(w.audit_event("execution.refused").await.is_none());
}

#[tokio::test]
async fn failed_run_settles_failed_and_creates_no_verification() {
    let w = world().await;
    w.verify_after_ten_seconds();
    w.link.on_skill(|_, _| Ok(failed_output(1)));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    let snapshot = w.until_outcome(op, step, StepOutcome::Failed).await;

    assert_eq!(reason(&w, op, step).await, "exit code 1");
    assert!(
        snapshot
            .steps
            .iter()
            .all(|record| record.step.kind != StepKind::Verification)
    );
    let output = snapshot.step(step).unwrap().progress.output.clone();
    assert!(output.unwrap_or_default().contains("no such container"));
}

#[tokio::test]
async fn verification_failure_starts_a_new_cycle() {
    let w = world().await;
    w.verify_after_ten_seconds();
    w.link.on_check(|_, _, _| Ok(firing()));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let snapshot = w.until_verified(op).await;

    let verification = snapshot
        .steps
        .iter()
        .find(|record| record.step.kind == StepKind::Verification)
        .unwrap();
    assert_eq!(verification.progress.outcome, StepOutcome::Failed);
    assert_eq!(snapshot.operation.cycle, 1);
    assert!(!snapshot.operation.status.is_terminal());
    let build: Vec<_> = w
        .effects(op, EffectStatus::Pending)
        .await
        .into_iter()
        .filter(|effect| effect.kind == "build_context")
        .collect();
    assert_eq!(
        build.len(),
        1,
        "a new cycle asks for its context, unclaimed"
    );
    assert_eq!(
        build[0].dedup_key.as_deref(),
        Some(format!("build:{op}:1").as_str())
    );
}

#[tokio::test]
async fn verification_pass_succeeds_the_operation() {
    let w = world().await;
    w.verify_after_ten_seconds();
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    let snapshot = w.until_verified(op).await;

    let verification = snapshot
        .steps
        .iter()
        .find(|record| record.step.kind == StepKind::Verification)
        .unwrap();
    assert_eq!(verification.progress.outcome, StepOutcome::Succeeded);
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert!(snapshot.operation.ended_at.is_some());
}

#[tokio::test]
async fn expired_authorization_settles_not_run() {
    let w = world_with(domovoi_engine::operations::OperationSettings {
        approval_ttl: SignedDuration::from_mins(10),
        ..domovoi_engine::operations::OperationSettings::default()
    })
    .await;
    let op = w.create().await;
    let step = w.propose(op).await;
    disconnect(&w);
    w.approve(op, step).await.unwrap();
    w.until_now(op, |s| s.operation.status == OperationStatus::WaitingAgent)
        .await;

    // The agent returns after the window the approval was valid for.
    reconnect(&w);
    w.advance(11 * 60);
    w.until_outcome(op, step, StepOutcome::NotRun).await;

    assert_eq!(
        reason(&w, op, step).await,
        "the approval expired before the action was dispatched"
    );
    assert!(w.skill_requests().is_empty());
    assert!(
        w.audit_kinds()
            .await
            .contains(&"execution.not_run".to_owned())
    );
}
