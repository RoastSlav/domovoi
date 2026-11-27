//! Investigations and the effect runner: a restart, a redelivered effect, the start-up
//! catch-up, and what keeps an investigation open or closes it.

use domovoi_core::ports::{
    AckRepo, EffectKind, EffectStatus, IncidentRepo, IncidentTimelineEntry, LlmError,
};
use domovoi_core::{
    AckRule, AckRuleId, Actor, CheckKey, EdgeKind, OperationId, OperationStatus, StepKind,
    StepOutcome, TargetPattern, VerifySpec,
};
use domovoi_engine::operations::OperationSettings;
use domovoi_testkit::approvals;
use domovoi_testkit::build::{T0, incident, open_incident};
use jiff::SignedDuration;
use serde_json::json;

use crate::drive::firing;
use crate::harness::{host_id, restart_skill};
use crate::inv::{Inv, InvSetup, disk, inv, of_kind, report, report_json, restart_report, turns};

/// Waits for the proposal to be stored and the operation to wait for a person.
async fn until_waiting(w: &Inv, op: OperationId) -> domovoi_core::OperationSnapshot {
    w.until(op, |s| {
        s.operation.status == OperationStatus::WaitingApproval
    })
    .await
}

/// An investigation whose report proposed one restart and is waiting for a person.
async fn proposed(w: &Inv) -> (OperationId, domovoi_core::StepId) {
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    let snapshot = until_waiting(w, op).await;
    w.until(op, |s| {
        of_kind(s, StepKind::Notification)
            .iter()
            .all(|n| n.progress.outcome == StepOutcome::Succeeded)
            && !of_kind(s, StepKind::Notification).is_empty()
    })
    .await;
    (op, of_kind(&snapshot, StepKind::Execution)[0].step.id)
}

fn two_proposals() -> String {
    report_json(
        "ACTION_NEEDED",
        json!([
            {"skill": "restart_container", "args": {"container": "nginx"}, "reason": "Stuck."},
            {"skill": "restart_container", "args": {"container": "redis"}, "reason": "Stuck."},
        ]),
    )
}

#[tokio::test]
async fn restart_resumes_an_unfinished_investigation() {
    let mut w = inv(InvSetup {
        held: vec![EffectKind::CallModel],
        ..InvSetup::default()
    })
    .await;
    w.llm.push_text(report());
    let op = w.start().await;
    w.jump(10).await;
    // BuildContext has committed the model call; the runner has not taken it.
    assert_eq!(
        w.effects(op, EffectStatus::Pending, Some(EffectKind::CallModel))
            .await
            .len(),
        1
    );
    assert!(w.llm.requests().is_empty());

    w.restart().await;
    w.gate.open();
    let snapshot = w.until_closed(op).await;

    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(
        w.count(&w.incident, "triage.report").await,
        1,
        "attached once"
    );
    assert_eq!(w.llm.requests().len(), 1);
    assert_eq!(w.operations_of(&w.incident).await.len(), 1);
}

#[tokio::test]
async fn redelivered_call_model_makes_no_second_model_call() {
    let w = inv(InvSetup::default()).await;
    let (op, _) = proposed(&w).await;
    assert_eq!(w.llm.requests().len(), 1);
    w.llm.push_text(report());

    w.redeliver(op, EffectKind::CallModel).await;

    assert_eq!(w.llm.requests().len(), 1, "one provider request in all");
    assert_eq!(turns(&w.load(op).await).len(), 1);
}

#[tokio::test]
async fn redelivered_draft_summary_creates_no_duplicate_steps() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(two_proposals());
    let op = w.start().await;
    w.until(op, |s| of_kind(s, StepKind::Execution).len() == 2)
        .await;
    w.until(op, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;

    w.redeliver(op, EffectKind::DraftSummary).await;
    w.settle().await;

    let snapshot = w.load(op).await;
    assert_eq!(of_kind(&snapshot, StepKind::Assessment).len(), 1);
    assert_eq!(of_kind(&snapshot, StepKind::Notification).len(), 1);
    assert_eq!(of_kind(&snapshot, StepKind::Execution).len(), 2);
    assert_eq!(of_kind(&snapshot, StepKind::Approval).len(), 2);
    assert!(w.effects(op, EffectStatus::Dead, None).await.is_empty());
}

#[tokio::test]
async fn redelivered_notify_attaches_again_but_finishes_once() {
    let w = inv(InvSetup::default()).await;
    let (op, _) = proposed(&w).await;
    assert_eq!(w.count(&w.incident, "triage.report").await, 1);

    w.redeliver(op, EffectKind::Notify).await;
    w.settle().await;

    // At-least-once: the outcome is attached again; the step is finished once.
    assert_eq!(w.count(&w.incident, "triage.report").await, 2);
    assert_eq!(w.completed().len(), 2);
    let snapshot = w.load(op).await;
    let notes = of_kind(&snapshot, StepKind::Notification);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].progress.outcome, StepOutcome::Succeeded);
    assert_eq!(w.llm.requests().len(), 1);
}

#[tokio::test]
async fn catch_up_investigates_open_incidents_without_a_triage_entry() {
    let mut w = inv(InvSetup {
        held: vec![EffectKind::BuildContext],
        ..InvSetup::default()
    })
    .await;
    let handled = w.add_incident("redis", disk(1.0)).await;
    IncidentRepo::append_event(
        &*w.store,
        &IncidentTimelineEntry {
            incident: handled.id(),
            at: T0,
            kind: "triage.report".to_owned(),
            summary: "already triaged".to_owned(),
            detail: serde_json::Value::Null,
        },
    )
    .await
    .unwrap();
    let pending = incident(host_id(), "docker.container", Some("pending")).unwrap();
    IncidentRepo::insert(&*w.store, &pending).await.unwrap();
    assert!(w.operations_of(&w.incident).await.is_empty());

    w.restart().await;

    assert_eq!(w.operations_of(&w.incident).await.len(), 1);
    assert!(
        w.operations_of(&handled).await.is_empty(),
        "already triaged"
    );
    assert!(w.operations_of(&pending).await.is_empty(), "still pending");
}

/// Reproduces the token flood after a restart: a listener a known condition covers already
/// has an open (quiet) incident, and the start-up pass used to investigate it anyway because
/// it looked only at the missing triage entry, never at the rules.
#[tokio::test]
async fn catch_up_skips_incidents_a_known_condition_covers() {
    let mut w = inv(InvSetup {
        held: vec![EffectKind::BuildContext],
        ..InvSetup::default()
    })
    .await;
    let covered = open_incident(host_id(), "security.new_port", Some("tcp:0.0.0.0:22")).unwrap();
    IncidentRepo::insert(&*w.store, &covered).await.unwrap();
    let uncovered =
        open_incident(host_id(), "security.new_port", Some("tcp:0.0.0.0:4444")).unwrap();
    IncidentRepo::insert(&*w.store, &uncovered).await.unwrap();
    AckRepo::put_rule(
        &*w.store,
        &AckRule {
            id: AckRuleId::new(),
            host: Some(host_id()),
            check: CheckKey::parse("security.new_port").unwrap(),
            target_pattern: Some(TargetPattern::parse("tcp:0.0.0.0:22").unwrap()),
            predicates: Vec::new(),
            note: "Found on first run (not reviewed)".to_owned(),
            expires_at: None,
            created_at: None,
            created_by: None,
        },
    )
    .await
    .unwrap();

    w.restart().await;

    assert!(
        w.operations_of(&covered).await.is_empty(),
        "a covered incident must not be investigated after a restart"
    );
    assert_eq!(
        w.operations_of(&uncovered).await.len(),
        1,
        "an unexplained one still is"
    );
}

#[tokio::test]
async fn closer_finishes_after_denial_expiry_and_failure() {
    // Denied: nothing is left open, and only the closer's pass notices.
    let w = inv(InvSetup::default()).await;
    let (op, step) = proposed(&w).await;
    w.ops
        .deny(op, step, approvals::human(w.user))
        .await
        .unwrap();
    w.settle().await;
    assert!(!w.load(op).await.operation.status.is_terminal());
    let snapshot = w.until_closed(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(
        snapshot.step(step).unwrap().progress.outcome,
        StepOutcome::Refused
    );

    // Expired: the approval window passes.
    let other = w.add_incident("redis", disk(2.0)).await;
    w.llm.push_text(restart_report("redis"));
    let expiring = w.ops.investigate(&other, false).await.unwrap();
    until_waiting(&w, expiring).await;
    let snapshot = w
        .until_stepping(expiring, 3600, |s| s.operation.status.is_terminal())
        .await;
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert!(
        of_kind(&snapshot, StepKind::Execution)
            .iter()
            .all(|e| e.progress.outcome.is_final()),
        "the proposal expired"
    );
    assert!(w.skill_requests() == 0, "an expired proposal never runs");

    // Failed: the model is down for every try.
    let third = w.add_incident("db", disk(3.0)).await;
    for _ in 0..3 {
        w.llm.push_err(LlmError::Unavailable("down".to_owned()));
    }
    let failing = w.ops.investigate(&third, false).await.unwrap();
    let snapshot = w
        .until_stepping(failing, 600, |s| s.operation.status.is_terminal())
        .await;
    assert_eq!(snapshot.operation.status, OperationStatus::Failed);
}

#[tokio::test]
async fn closer_never_closes_a_paused_or_queued_investigation() {
    let w = inv(InvSetup {
        ops: OperationSettings {
            max_running: 1,
            ..OperationSettings::default()
        },
        held: vec![EffectKind::BuildContext],
        ..InvSetup::default()
    })
    .await;

    // Paused: nothing is asked of the model while it is paused, and it is not closed.
    let paused = w.start().await;
    w.ops.pause(paused, Actor::User(w.user)).await.unwrap();
    w.gate.open();
    w.jump(600).await;
    let snapshot = w.load(paused).await;
    assert!(matches!(
        snapshot.operation.status,
        OperationStatus::Paused { .. }
    ));
    assert!(w.llm.requests().is_empty());

    // Queued: the paused one still holds the only slot.
    let third = w.add_incident("db", disk(3.0)).await;
    let queued = w.ops.investigate(&third, false).await.unwrap();
    w.jump(600).await;
    assert_eq!(
        w.load(queued).await.operation.status,
        OperationStatus::Queued
    );
    assert!(w.llm.requests().is_empty(), "nothing asked the model");

    // Resumed, the paused one finishes.
    w.llm.push_text(report());
    w.ops
        .resume(paused, Actor::User(w.user), Vec::new())
        .await
        .unwrap();
    let snapshot = w.until_closed(paused).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
}

#[tokio::test]
async fn queued_operation_defers_build_context() {
    let w = inv(InvSetup {
        ops: OperationSettings {
            max_running: 1,
            ..OperationSettings::default()
        },
        held: vec![EffectKind::CallModel],
        ..InvSetup::default()
    })
    .await;
    // The first operation is held at its model call: it is running and keeps the only slot.
    w.llm.push_text(restart_report("nginx"));
    let first = w.start().await;
    w.jump(10).await;
    let other = w.add_incident("redis", disk(2.0)).await;
    w.llm.push_text(report());
    let second = w.ops.investigate(&other, false).await.unwrap();

    w.jump(120).await;
    assert_eq!(
        w.load(second).await.operation.status,
        OperationStatus::Queued
    );
    let pending = w
        .effects(
            second,
            EffectStatus::Pending,
            Some(EffectKind::BuildContext),
        )
        .await;
    assert_eq!(
        pending.len(),
        1,
        "its context build is deferred, not dropped"
    );

    // The first one reaches its approval wait and frees the slot: the second is admitted.
    w.gate.open();
    until_waiting(&w, first).await;
    let snapshot = w.until_closed(second).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(w.count(&other, "triage.report").await, 1);
}

#[tokio::test]
async fn an_operation_waiting_for_a_person_does_not_hold_a_slot() {
    let w = inv(InvSetup {
        ops: OperationSettings {
            max_running: 1,
            ..OperationSettings::default()
        },
        ..InvSetup::default()
    })
    .await;
    let (first, _step) = proposed(&w).await;
    assert_eq!(
        w.load(first).await.operation.status,
        OperationStatus::WaitingApproval
    );
    let other = w.add_incident("redis", disk(2.0)).await;
    w.llm.push_text(report());
    let second = w.ops.investigate(&other, false).await.unwrap();
    let snapshot = w.until_closed(second).await;
    assert_eq!(
        snapshot.operation.status,
        OperationStatus::Succeeded,
        "one operation waiting for approval must not starve the queue"
    );
}

#[tokio::test]
async fn verification_failure_runs_a_second_investigation_cycle() {
    let w = inv(InvSetup::default()).await;
    let mut skill = restart_skill();
    skill.verify = Some(VerifySpec {
        recheck: true,
        delay: SignedDuration::from_secs(10),
    });
    w.link.set_skills(host_id(), vec![skill.clone()]);
    w.caps.set_skills(host_id(), vec![skill]);
    w.link.on_check(|_, _, _| Ok(firing()));
    let (op, step) = proposed(&w).await;
    w.llm.push_text(report());

    w.approve(op, step).await.unwrap();
    let snapshot = w
        .until(op, |s| {
            s.operation.cycle == 1
                && turns(s).len() == 2
                && turns(s).iter().all(|t| t.progress.outcome.is_final())
        })
        .await;

    assert_eq!(w.skill_requests(), 1);
    let verification = of_kind(&snapshot, StepKind::Verification);
    assert_eq!(verification.len(), 1);
    assert_eq!(verification[0].progress.outcome, StepOutcome::Failed);
    let second = turns(&snapshot)[1];
    assert_eq!(second.step.cycle, 1);
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::CausedBy
                && edge.from == second.step.id
                && edge.to == verification[0].step.id),
        "the new turn is caused by the verification"
    );
    w.until(op, |s| {
        of_kind(s, StepKind::Notification)
            .iter()
            .filter(|n| n.progress.outcome == StepOutcome::Succeeded)
            .count()
            == 2
    })
    .await;
    assert_eq!(
        w.count(&w.incident, "triage.report").await,
        2,
        "a second report"
    );
    assert_eq!(w.llm.requests().len(), 2);
}
