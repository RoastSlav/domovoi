//! The operation side of the meta-tools: reads recorded as evidence steps in the turn commit,
//! staged steps checked again and stored, and a question that stops and resumes an
//! investigation.

use domovoi_core::ports::{
    AggregateRef, EffectKind, JournalRead, LlmError, LlmResponse, Part, Presence, StopReason,
    Usage, UsageSource,
};
use domovoi_core::{
    Actor, Availability, BudgetLimits, ContractId, EdgeKind, OperationId, OperationSnapshot,
    OperationStatus, PauseReason, SkillName, StepId, StepIntent, StepKind, StepOutcome, StepRecord,
    UserId,
};
use domovoi_engine::operations::{DecideError, NewEdge, OperationEvent, RecordedStep};
use serde_json::{Value, json};

use crate::harness::host_id;
use crate::inv::{Inv, InvSetup, inv, of_kind, report, report_json, restart_report, turns};
use crate::support::Fixture;

/// A model turn that calls tools: `(id, name, input)` each.
fn calls(list: &[(&str, &str, Value)]) -> LlmResponse {
    LlmResponse {
        parts: list
            .iter()
            .map(|(id, name, input)| Part::ToolCall {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                input: input.clone(),
            })
            .collect(),
        stop: StopReason::ToolUse,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            cost_micros: Some(10),
            source: UsageSource::Reported,
        },
    }
}

/// A turn that reads with `request_evidence`: `(id, read tool, args)` each.
fn reads(list: &[(&str, &str, Value)]) -> LlmResponse {
    let wrapped: Vec<(&str, &str, Value)> = list
        .iter()
        .map(|(id, tool, args)| (*id, "request_evidence", json!({"tool": tool, "args": args})))
        .collect();
    calls(&wrapped)
}

/// A turn that asks `question` and ends there.
fn ask(question: &str) -> LlmResponse {
    calls(&[("q", "ask_user", json!({"question": question}))])
}

/// A `propose_plan` call and a `finish` call, in one response.
fn plan_and_finish() -> LlmResponse {
    let plan = json!({"steps": [{
        "contract": "skill.restart_container",
        "args": {"container": "nginx"},
        "reason": "It is stuck."
    }]});
    let mut finish: Value = serde_json::from_str(&report()).unwrap();
    finish.as_object_mut().unwrap().remove("proposals");
    calls(&[("p", "propose_plan", plan), ("f", "finish", finish)])
}

/// The evidence steps of a snapshot, in order.
fn evidence(snapshot: &OperationSnapshot) -> Vec<&StepRecord> {
    of_kind(snapshot, StepKind::EvidenceRequest)
}

fn recorded(fx: &mut Fixture, tool: &str, outcome: StepOutcome) -> RecordedStep {
    RecordedStep {
        id: fx.fresh_step(),
        host: fx.subject.host,
        tool: tool.to_owned(),
        args: json!({}),
        outcome,
        summary: Some("seen".to_owned()),
        reason: None,
        output: Some("the text".to_owned()),
    }
}

fn turn_finished(step: StepId, evidence: Vec<RecordedStep>) -> OperationEvent {
    OperationEvent::TurnFinished {
        step,
        outcome: StepOutcome::Succeeded,
        summary: Some("done".to_owned()),
        reason: None,
        output: Some("{}".to_owned()),
        usage: None,
        evidence,
    }
}

fn running_turn(fx: &mut Fixture) -> StepId {
    let step = fx.new_step(StepIntent::ReasoningTurn {
        context_refs: Vec::new(),
    });
    fx.propose(vec![step])[0]
}

/// Proposes a question caused by `turn` and returns its id.
fn question_of(fx: &mut Fixture, turn: StepId) -> StepId {
    let question = fx.question(3600);
    let id = question.id;
    fx.event(OperationEvent::Propose {
        steps: vec![question],
        edges: vec![NewEdge {
            from: id,
            to: turn,
            kind: EdgeKind::CausedBy,
        }],
    });
    id
}

/// The effects an answer to `question` creates.
fn answer_effects(fx: &mut Fixture, question: StepId) -> Vec<EffectKind> {
    let note = fx.note();
    fx.event(OperationEvent::Answer { question, note })
        .effects
        .iter()
        .map(|effect| effect.kind)
        .collect()
}

#[tokio::test]
async fn evidence_calls_become_evidence_steps_in_the_turn_commit() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(reads(&[
        ("a", "docker_ps", json!({})),
        ("b", "docker_ps", json!({"all": true})),
    ]));
    w.llm.push_text(report());
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    let steps = evidence(&snapshot);
    assert_eq!(steps.len(), 2);
    let turn = turns(&snapshot)[0].step.id;
    for record in &steps {
        assert_eq!(record.step.phase, "evidence");
        assert_eq!(record.step.parent, Some(turn));
        assert_eq!(record.progress.outcome, StepOutcome::Succeeded);
        assert_eq!(record.progress.started_at, record.progress.ended_at);
        assert!(
            snapshot
                .edges
                .iter()
                .any(|edge| edge.kind == EdgeKind::CausedBy
                    && edge.from == record.step.id
                    && edge.to == turn)
        );
    }
    assert_eq!(snapshot.operation.budgets.used.evidence_requests, 2);
    let journal = JournalRead::entries_for(&*w.store, &AggregateRef::operation(op), 500)
        .await
        .unwrap();
    for record in &steps {
        let id = record.step.id.to_string();
        let kinds: Vec<&str> = journal
            .iter()
            .filter(|entry| entry.payload["step"] == id)
            .map(|entry| entry.kind.as_str())
            .collect();
        assert!(kinds.contains(&"operation.step_created"), "{kinds:?}");
        assert!(kinds.contains(&"operation.step_finished"), "{kinds:?}");
    }
    for entry in &journal {
        assert!(
            entry.payload.get("output").is_none(),
            "a journal row never carries output: {} {}",
            entry.kind,
            entry.payload
        );
    }
    assert!(
        w.all_effects(op, EffectKind::CollectEvidence)
            .await
            .is_empty(),
        "reads are recorded, not collected"
    );
}

#[tokio::test]
async fn missing_evidence_is_recorded_as_an_unavailable_step() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(reads(&[("a", "docker_ps", json!({}))]));
    w.llm.push_text(report());
    w.link
        .set_presence(host_id(), Presence::Disconnected { last_seen: None });
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    let steps = evidence(&snapshot);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].progress.outcome, StepOutcome::Unavailable);
    assert!(
        steps[0]
            .progress
            .reason
            .as_deref()
            .is_some_and(|reason| !reason.is_empty())
    );
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
}

#[tokio::test]
async fn refused_evidence_does_not_charge_the_budget() {
    let w = inv(InvSetup::default()).await;
    let ids: Vec<String> = (0..20).map(|n| format!("c{n}")).collect();
    let list: Vec<(&str, &str, Value)> = ids
        .iter()
        .map(|id| (id.as_str(), "docker_ps", json!({})))
        .collect();
    w.llm.push_ok(reads(&list));
    w.llm.push_text(report());
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    let steps = evidence(&snapshot);
    let refused = steps
        .iter()
        .filter(|record| record.progress.outcome == StepOutcome::Refused)
        .count();
    assert_eq!(steps.len(), 20);
    assert_eq!(refused, 5, "15 calls run, the other 5 are refused");
    assert_eq!(snapshot.operation.budgets.used.evidence_requests, 15);
}

#[tokio::test]
async fn an_exhausted_evidence_budget_refuses_without_pausing() {
    let mut ops = domovoi_engine::operations::OperationSettings::default();
    ops.budgets.evidence_requests = Some(1);
    let w = inv(InvSetup {
        ops,
        ..InvSetup::default()
    })
    .await;
    w.llm.push_ok(reads(&[
        ("a", "docker_ps", json!({})),
        ("b", "docker_ps", json!({"all": true})),
    ]));
    w.llm.push_text(report());
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    let outcomes: Vec<StepOutcome> = evidence(&snapshot)
        .iter()
        .map(|record| record.progress.outcome)
        .collect();
    assert_eq!(outcomes, vec![StepOutcome::Succeeded, StepOutcome::Refused]);
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    let journal = JournalRead::entries_for(&*w.store, &AggregateRef::operation(op), 500)
        .await
        .unwrap();
    assert!(
        journal.iter().all(|entry| entry.payload["to"] != "paused"),
        "the loop never pauses the operation"
    );
}

#[tokio::test]
async fn a_failed_turn_still_records_its_evidence() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(reads(&[
        ("a", "docker_ps", json!({})),
        ("b", "docker_ps", json!({"all": true})),
    ]));
    w.llm.push_err(LlmError::Unavailable("down".to_owned()));
    let op = w.start().await;
    let snapshot = w
        .until(op, |s| {
            turns(s)
                .first()
                .is_some_and(|turn| turn.progress.outcome == StepOutcome::Failed)
        })
        .await;

    assert_eq!(evidence(&snapshot).len(), 2);
}

#[test]
fn turn_finished_is_refused_for_a_terminal_operation_and_a_turn_that_is_not_running() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let turn = running_turn(&mut fx);
    let read = recorded(&mut fx, "docker_ps", StepOutcome::Succeeded);
    let note = fx.note();
    let note = fx.propose(vec![note])[0];
    assert!(matches!(
        fx.try_event(turn_finished(note, Vec::new())),
        Err(DecideError::Invalid(_))
    ));

    fx.event(OperationEvent::Pause {
        reason: PauseReason::Manual,
    });
    let decision = fx.event(turn_finished(turn, vec![read]));
    assert_eq!(decision.steps.len(), 1, "accepted while paused");
    assert!(matches!(
        fx.try_event(turn_finished(turn, Vec::new())),
        Err(DecideError::StepNotOpen(_))
    ));

    let mut fx = Fixture::running(BudgetLimits::default());
    let turn = running_turn(&mut fx);
    fx.event(OperationEvent::Cancel {
        reason: "stop".to_owned(),
    });
    assert!(fx.try_event(turn_finished(turn, Vec::new())).is_err());
}

#[test]
fn an_evidence_record_that_does_not_validate_is_invalid() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let turn = running_turn(&mut fx);
    let bad = recorded(&mut fx, "rm_rf", StepOutcome::Succeeded);
    let before = fx.state.snapshot().steps.clone();

    assert!(matches!(
        fx.try_event(turn_finished(turn, vec![bad])),
        Err(DecideError::Invalid(_))
    ));
    assert_eq!(fx.state.snapshot().steps, before, "nothing was written");
    let pending = recorded(&mut fx, "docker_ps", StepOutcome::Pending);
    assert!(matches!(
        fx.try_event(turn_finished(turn, vec![pending])),
        Err(DecideError::Invalid(_))
    ));
}

#[test]
fn refused_records_are_not_charged_and_create_no_effect() {
    let limits = BudgetLimits {
        evidence_requests: Some(1),
        ..BudgetLimits::default()
    };
    let mut fx = Fixture::running(limits);
    let turn = running_turn(&mut fx);
    let records = vec![
        recorded(&mut fx, "docker_ps", StepOutcome::Succeeded),
        recorded(&mut fx, "docker_ps", StepOutcome::Refused),
        recorded(&mut fx, "docker_ps", StepOutcome::Refused),
    ];
    let decision = fx.event(turn_finished(turn, records));

    assert_eq!(fx.state.operation().budgets.used.evidence_requests, 1);
    assert!(decision.effects.is_empty(), "no collect_evidence effect");
    assert!(!matches!(fx.status(), OperationStatus::Paused { .. }));
}

#[tokio::test]
async fn staged_steps_are_validated_again_when_they_are_stored() {
    let w = inv(InvSetup {
        held: vec![EffectKind::DraftSummary],
        ..InvSetup::default()
    })
    .await;
    w.llm.push_ok(plan_and_finish());
    let op = w.start().await;
    w.until(op, |s| {
        turns(s)[0].progress.outcome == StepOutcome::Succeeded
    })
    .await;
    let contract = ContractId::skill(&SkillName::parse("restart_container").unwrap());
    w.caps.set_availability(
        host_id(),
        &contract,
        Availability::HostAbsent { since: w.now() },
    );
    w.gate.open();
    let snapshot = w.until_closed(op).await;

    assert!(of_kind(&snapshot, StepKind::Execution).is_empty());
    let shown = format!("{:?}", w.last_outcome());
    assert!(shown.contains("restart_container: "), "{shown}");
    assert!(shown.contains("not connected"), "{shown}");
}

#[tokio::test]
async fn staged_steps_are_stored_through_propose_execution() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(plan_and_finish());
    let op = w.start().await;
    let snapshot = w
        .until(op, |s| !of_kind(s, StepKind::Execution).is_empty())
        .await;

    assert_eq!(of_kind(&snapshot, StepKind::Approval).len(), 1);
    assert!(w.actions().await.is_empty(), "no row in actions");
    let executions = of_kind(&snapshot, StepKind::Execution);
    assert_eq!(executions.len(), 1);
    let StepIntent::Execution(intent) = &executions[0].step.intent else {
        panic!("an execution");
    };
    assert_eq!(intent.contract.to_string(), "skill.restart_container");
    assert_eq!(
        intent.args.get("container").map(String::as_str),
        Some("nginx")
    );
    assert!(intent.retry_of.is_none());
}

#[tokio::test]
async fn cached_report_proposals_go_through_the_validator() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "wipe_disk", "args": {}, "reason": "Make room."}]),
    ));
    let op = w.start().await;
    let snapshot = w.until_closed(op).await;

    assert!(of_kind(&snapshot, StepKind::Execution).is_empty());
    let shown = format!("{:?}", w.last_outcome());
    assert!(
        shown.contains("wipe_disk: not a skill on this host"),
        "{shown}"
    );
}

#[tokio::test]
async fn a_turn_output_written_by_the_previous_version_is_processed() {
    use domovoi_core::ports::{OperationWrite, StateWrite, Transaction, UnitOfWork};
    let w = inv(InvSetup {
        held: vec![EffectKind::DraftSummary],
        ..InvSetup::default()
    })
    .await;
    w.llm.push_text(restart_report("nginx"));
    let op = w.start().await;
    let held = w
        .until(op, |s| {
            turns(s)[0].progress.outcome == StepOutcome::Succeeded
        })
        .await;
    assert!(of_kind(&held, StepKind::Execution).is_empty());
    // Strip every field the previous version never wrote.
    let turn = turns(&held)[0].clone();
    let mut output: Value = serde_json::from_str(turn.progress.output.as_deref().unwrap()).unwrap();
    for key in ["question", "plan", "dropped"] {
        output.as_object_mut().unwrap().remove(key);
    }
    let mut progress = turn.progress.clone();
    progress.output = Some(output.to_string());
    let write = OperationWrite::Progress {
        step: turn.step.id,
        from: StepOutcome::Succeeded,
        to: progress,
    };
    UnitOfWork::commit(
        &*w.store,
        Transaction::new().write(StateWrite::Operation(write)),
    )
    .await
    .unwrap();
    w.gate.open();
    let snapshot = w
        .until(op, |s| !of_kind(s, StepKind::Execution).is_empty())
        .await;

    assert_eq!(of_kind(&snapshot, StepKind::Execution).len(), 1);
    assert_eq!(of_kind(&snapshot, StepKind::Notification).len(), 1);
}

#[tokio::test]
async fn an_asked_turn_stores_a_question_and_no_notification() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(ask("Is the config file yours?"));
    let op = w.start().await;
    let snapshot = w
        .until(op, |s| !of_kind(s, StepKind::Question).is_empty())
        .await;

    let question = of_kind(&snapshot, StepKind::Question)[0];
    let StepIntent::Question { text, expires_at } = &question.step.intent else {
        panic!("a question");
    };
    assert_eq!(text, "Is the config file yours?");
    assert_eq!(
        *expires_at,
        question.step.created_at + w.settings.question_timeout
    );
    assert_eq!(question.step.phase, "ask");
    assert!(of_kind(&snapshot, StepKind::Notification).is_empty());
    assert!(of_kind(&snapshot, StepKind::Assessment).is_empty());
    assert!(of_kind(&snapshot, StepKind::Plan).is_empty());
    assert!(w.all_effects(op, EffectKind::Notify).await.is_empty());
    assert_eq!(snapshot.operation.status, OperationStatus::WaitingUser);
}

async fn answered(w: &Inv, op: OperationId, text: &str) {
    let snapshot = w.load(op).await;
    let question = of_kind(&snapshot, StepKind::Question)[0].step.id;
    w.ops
        .answer(op, question, text.to_owned(), Actor::User(UserId::new()))
        .await
        .unwrap();
}

#[tokio::test]
async fn answering_the_question_resumes_the_investigation() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(ask("Is the config file yours?"));
    w.llm.push_text(report());
    let op = w.start().await;
    w.until(op, |s| !of_kind(s, StepKind::Question).is_empty())
        .await;
    answered(&w, op, "Yes, I edited it today.").await;
    let snapshot = w.until_closed(op).await;

    let resumes: Vec<_> = w
        .all_effects(op, EffectKind::BuildContext)
        .await
        .into_iter()
        .filter(|effect| effect.payload.get("resume").is_some())
        .collect();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].payload["resume"], "answer");
    let requests = w.llm.requests();
    assert_eq!(requests.len(), 2);
    let first = format!("{:?}", requests[1].messages[0]);
    assert!(first.contains("## This investigation so far"), "{first}");
    assert!(first.contains("Yes, I edited it today."), "{first}");
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(turns(&snapshot).len(), 2);
    let question = of_kind(&snapshot, StepKind::Question)[0].step.id;
    let resumed = turns(&snapshot)[1].step.id;
    assert!(
        snapshot
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::CausedBy
                && edge.from == resumed
                && edge.to == question)
    );
    assert_eq!(w.count(&w.incident, "triage.report").await, 1);
}

#[tokio::test]
async fn a_question_timeout_resumes_with_the_no_answer_sentence() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_ok(ask("Is the config file yours?"));
    w.llm.push_text(report());
    let op = w.start().await;
    w.until(op, |s| !of_kind(s, StepKind::Question).is_empty())
        .await;
    w.jump(w.settings.question_timeout.as_secs() + 1).await;
    let snapshot = w.until_closed(op).await;

    let requests = w.llm.requests();
    assert_eq!(requests.len(), 2);
    let first = format!("{:?}", requests[1].messages[0]);
    assert!(
        first.contains("No answer arrived before the timeout."),
        "{first}"
    );
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
}

#[test]
fn a_resume_creates_nothing_when_a_later_turn_exists() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let turn = running_turn(&mut fx);
    let question = question_of(&mut fx, turn);
    assert!(
        !answer_effects(&mut fx, question).contains(&EffectKind::BuildContext),
        "the asking turn has not ended"
    );
    fx.event(turn_finished(turn, Vec::new()));

    let turn_two = running_turn(&mut fx);
    fx.event(turn_finished(turn_two, Vec::new()));
    let second = question_of(&mut fx, turn_two);
    let note = fx.note();
    let decision = fx.event(OperationEvent::Answer {
        question: second,
        note,
    });
    let resumes: Vec<_> = decision
        .effects
        .iter()
        .filter(|effect| effect.kind == EffectKind::BuildContext)
        .collect();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].payload["resume"], "answer");
    assert_eq!(resumes[0].payload["question"], second.to_string());
    let expected = format!("build:{}:0:resume:{second}", fx.state.operation().id);
    assert_eq!(resumes[0].dedup_key.as_deref(), Some(expected.as_str()));

    // A later turn exists: another question of the old turn resumes nothing.
    running_turn(&mut fx);
    let stale = question_of(&mut fx, turn_two);
    assert!(!answer_effects(&mut fx, stale).contains(&EffectKind::BuildContext));
}

#[test]
fn a_timed_out_question_resumes_with_the_timeout_payload() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let turn = running_turn(&mut fx);
    fx.event(turn_finished(turn, Vec::new()));
    let question = question_of(&mut fx, turn);
    let decision = fx.event(OperationEvent::StepFinished {
        step: question,
        outcome: StepOutcome::NotRun,
        summary: None,
        reason: Some("no answer before the timeout".to_owned()),
        output: None,
        usage: None,
    });
    let resumes: Vec<_> = decision
        .effects
        .iter()
        .filter(|effect| effect.kind == EffectKind::BuildContext)
        .collect();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].payload["resume"], "timeout");
}

#[tokio::test]
async fn ask_user_is_not_offered_when_the_service_does_not_allow_questions() {
    let w = inv(InvSetup {
        questions: false,
        ..InvSetup::default()
    })
    .await;
    w.llm.push_text(report());
    let op = w.start().await;
    w.until_closed(op).await;

    let requests = w.llm.requests();
    assert!(!requests.is_empty());
    for request in requests {
        let names: Vec<String> = request.tools.iter().map(|tool| tool.name.clone()).collect();
        assert!(!names.iter().any(|name| name == "ask_user"), "{names:?}");
        assert!(names.iter().any(|name| name == "finish"), "{names:?}");
    }
}
