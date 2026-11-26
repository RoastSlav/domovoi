//! Investigate operations: starting, the report, proposals as steps and what the report
//! tells its reader.

use domovoi_core::ports::EffectKind;
use domovoi_core::{
    EdgeKind, ExecutionId, OperationStatus, PinnedSubject, StepKind, StepOutcome, TriageOutcome,
};
use domovoi_engine::operations::{APPROVAL_POINTER, InvestigateError};
use domovoi_engine::{DomainEvent, EventKind};
use serde_json::json;

use crate::harness::host_id;
use crate::inv::{
    InvSetup, answer_with_usage, check, inv, of_kind, report, report_json, restart_report, target,
    turns,
};

/// The report attached last, which must be a report.
fn attached_report(outcome: TriageOutcome) -> domovoi_core::TriageReport {
    match outcome {
        TriageOutcome::Report(report) => report,
        other => panic!("expected a report, got {other:?}"),
    }
}

#[tokio::test]
async fn seed_pins_exactly_the_incident_subject() {
    let w = inv(InvSetup {
        held: vec![EffectKind::BuildContext],
        ..InvSetup::default()
    })
    .await;
    let op = w.start().await;
    let snapshot = w.load(op).await;

    assert_eq!(
        snapshot.operation.pinned,
        vec![PinnedSubject {
            host: host_id(),
            check: check(),
            target: Some(target("nginx")),
        }]
    );
    let limits = &snapshot.operation.budgets.limits;
    assert_eq!(limits.tokens, None);
    assert_eq!(limits.cost_micros, None);
    assert_eq!(limits.model_turns, w.settings.budgets.model_turns);
    let first = turns(&snapshot);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].progress.outcome, StepOutcome::Running);
    assert_eq!(snapshot.steps.len(), 1);
    assert_eq!(snapshot.incidents, vec![w.incident.id()]);
}

#[tokio::test]
async fn incident_event_starts_one_investigation() {
    let w = inv(InvSetup {
        held: vec![EffectKind::BuildContext],
        ..InvSetup::default()
    })
    .await;
    let event = DomainEvent::new(EventKind::TriageRequested, &w.incident, true);
    w.bus.send(event.clone()).unwrap();
    w.bus.send(event).unwrap();
    w.settle().await;
    let quiet = DomainEvent::new(EventKind::TriageRequested, &w.incident, false);
    w.bus.send(quiet).unwrap();
    w.settle().await;

    let found = w.operations_of(&w.incident).await;
    assert_eq!(found.len(), 1, "one investigation however many requests");
    let snapshot = w.load(found[0].id).await;
    assert_eq!(snapshot.incidents, vec![w.incident.id()]);
}

#[tokio::test]
async fn retriage_while_open_is_refused() {
    let w = inv(InvSetup {
        held: vec![EffectKind::BuildContext],
        ..InvSetup::default()
    })
    .await;
    let open = w.start().await;

    let refused = w.ops.investigate_and_wait(&w.incident, true).await;

    assert_eq!(refused, Err(InvestigateError::AlreadyRunning(open)));
    assert_eq!(w.operations_of(&w.incident).await.len(), 1);
}

#[tokio::test]
async fn investigation_delivers_the_report_and_closes() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(report());
    let waiting = w.spawn_wait();
    w.settle().await;
    let op = w.operations_of(&w.incident).await[0].id;

    let snapshot = w.until_closed(op).await;

    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    let first = turns(&snapshot);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].progress.outcome, StepOutcome::Succeeded);
    assert_eq!(of_kind(&snapshot, StepKind::Assessment).len(), 1);
    let notes = of_kind(&snapshot, StepKind::Notification);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].progress.outcome, StepOutcome::Succeeded);
    assert_eq!(w.count(&w.incident, "triage.report").await, 1);
    assert_eq!(w.llm.requests().len(), 1);
    let waited = waiting.await.unwrap().unwrap();
    assert!(matches!(waited, TriageOutcome::Report(r) if r.headline == "nginx keeps restarting"));
}

#[tokio::test]
async fn usage_is_recorded_on_the_turn_and_never_fabricated() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(answer_with_usage(report(), 100, 10));
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;
    assert_eq!(snapshot.operation.budgets.used.tokens, 110);
    assert_eq!(snapshot.operation.budgets.used.model_turns, 1);

    // A run that got no response has no usage to record.
    let other = w.add_incident("redis", crate::inv::disk(50.0)).await;
    w.llm.push_err(domovoi_core::ports::LlmError::Unavailable(
        "down".to_owned(),
    ));
    let failed = w.ops.investigate(&other, false).await.unwrap();
    let snapshot = w
        .until(failed, |s| {
            turns(s)
                .iter()
                .any(|t| t.progress.outcome == StepOutcome::Failed)
        })
        .await;
    assert_eq!(snapshot.operation.budgets.used.tokens, 0);
    assert_eq!(snapshot.operation.budgets.used.cost_micros, 0);
}

#[tokio::test]
async fn proposal_becomes_an_execution_step_with_a_plan_hash() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    let snapshot = w
        .until(op, |s| {
            s.operation.status == OperationStatus::WaitingApproval
        })
        .await;

    let executions = of_kind(&snapshot, StepKind::Execution);
    assert_eq!(executions.len(), 1);
    let step = executions[0].step.id;
    let row = snapshot.execution_of(step).expect("an executions row");
    assert_eq!(row.value.execution, ExecutionId::from_step(step));
    assert_eq!(row.value.step, step);
    assert_eq!(row.value.args["container"], "nginx");
    assert_eq!(of_kind(&snapshot, StepKind::Approval).len(), 1);
    let approval = of_kind(&snapshot, StepKind::Approval)[0].step.id;
    let turn = turns(&snapshot)[0].step.id;
    let has = |from, to, kind| {
        snapshot
            .edges
            .iter()
            .any(|e| e.from == from && e.to == to && e.kind == kind)
    };
    assert!(
        has(step, approval, EdgeKind::DependsOn),
        "needs its approval"
    );
    assert!(has(step, turn, EdgeKind::CausedBy), "caused by the turn");
    assert_eq!(
        w.skill_requests(),
        0,
        "nothing runs before a person approves"
    );
}

#[tokio::test]
async fn proposal_for_an_unknown_skill_is_dropped_with_a_line() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "wipe_disk", "args": {}, "reason": "Make room."}]),
    ));
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    assert!(of_kind(&snapshot, StepKind::Execution).is_empty());
    let report = attached_report(w.last_outcome());
    assert!(report.proposals.is_empty());
    assert!(
        report
            .details
            .ends_with("Proposals not offered:\n- wipe_disk: not a skill on this host"),
        "{}",
        report.details
    );
}

#[tokio::test]
async fn refused_execution_is_listed_as_not_offered() {
    let w = inv(InvSetup::default()).await;
    // Triage's registry still enables the skill, but the agent session no longer offers it,
    // so the writer stores the proposal refused.
    w.link.set_skills(host_id(), Vec::new());
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    let executions = of_kind(&snapshot, StepKind::Execution);
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0].progress.outcome, StepOutcome::Refused);
    assert!(of_kind(&snapshot, StepKind::Approval).is_empty());
    let report = attached_report(w.last_outcome());
    assert!(
        report.proposals.is_empty(),
        "a refused proposal is not offered"
    );
    assert!(
        report.details.contains(
            "Proposals not offered:\n- restart_container: the agent does not offer this skill"
        ),
        "{}",
        report.details
    );
    assert!(!report.details.contains(APPROVAL_POINTER));
}

#[tokio::test]
async fn report_tells_the_reader_where_to_approve() {
    // A step with chat buttons needs no pointer; one without buttons does.
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    w.until(op, |s| {
        s.operation.status == OperationStatus::WaitingApproval
    })
    .await;
    let buttoned = attached_report(w.last_outcome());
    assert!(
        !buttoned.details.contains(APPROVAL_POINTER),
        "{}",
        buttoned.details
    );
    assert_eq!(buttoned.proposals.len(), 1);

    let w = inv(InvSetup::default()).await;
    let mut skill = crate::harness::restart_skill();
    skill.risk = domovoi_core::Risk::Destructive;
    w.link.set_skills(crate::harness::host_id(), vec![skill]);
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    w.until(op, |s| {
        s.operation.status == OperationStatus::WaitingApproval
    })
    .await;
    let pending = attached_report(w.last_outcome());
    assert!(
        pending.details.ends_with(APPROVAL_POINTER),
        "{}",
        pending.details
    );
    assert_eq!(pending.proposals.len(), 1);

    let other = w.add_incident("redis", crate::inv::disk(50.0)).await;
    w.llm.push_text(report());
    let plain = w.ops.investigate(&other, false).await.unwrap();
    w.until_closed(plain).await;
    let none = attached_report(w.last_outcome());
    assert!(!none.details.contains(APPROVAL_POINTER));
}

#[tokio::test]
async fn notify_mints_one_token_per_chat_capable_step_and_passes_them_to_the_card() {
    use domovoi_core::ports::OperationRepo;
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    w.until(op, |s| {
        s.operation.status == OperationStatus::WaitingApproval
    })
    .await;

    let completed = w.completed();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].1, 1, "one token for the one buttoned step");
    assert!(matches!(&completed[0].0, TriageOutcome::Report(r) if r.proposals.len() == 1));
    let tokens = w
        .events()
        .into_iter()
        .find_map(|event| match event.kind {
            EventKind::TriageCompleted { tokens, .. } => Some(tokens),
            _ => None,
        })
        .unwrap();
    let (execution, token) = tokens.iter().next().unwrap();
    assert_eq!(token.len(), 64);
    let stored = OperationRepo::execution(&*w.store, *execution)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.value.chat_token_hash(),
        Some(&domovoi_engine::chat_token::token_hash(token))
    );

    // A destructive step never gets a token.
    let w = inv(InvSetup::default()).await;
    let mut skill = crate::harness::restart_skill();
    skill.risk = domovoi_core::Risk::Destructive;
    w.link.set_skills(crate::harness::host_id(), vec![skill]);
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    w.until(op, |s| {
        s.operation.status == OperationStatus::WaitingApproval
    })
    .await;
    assert_eq!(w.completed()[0].1, 0, "no token for a destructive step");
}
