//! `decide` for the operation as a whole and for the steps a caller may propose.

use domovoi_core::ports::{EffectKind, JournalKind};
use domovoi_core::{
    BudgetKind, BudgetLimits, EdgeKind, OperationStatus, PauseReason, StepIntent, StepKind,
    StepOutcome, StepUsage,
};
use domovoi_engine::operations::{DecideError, NewEdge, NewStep, OperationEvent};
use jiff::SignedDuration;

use crate::support::{Fixture, hash};

fn no_limits() -> BudgetLimits {
    BudgetLimits::default()
}

#[test]
fn admit_moves_queued_to_running_and_sets_started_at() {
    let mut fx = Fixture::new(no_limits());
    let decision = fx.event(OperationEvent::Admit);
    assert_eq!(fx.status(), OperationStatus::Running);
    assert_eq!(fx.state.operation().started_at, Some(fx.now));
    assert_eq!(decision.operation.updated_at, fx.now);
    assert!(
        decision
            .journal
            .iter()
            .any(|n| n.kind == JournalKind::OperationStatus && n.payload["to"] == "running")
    );
    let again = fx.try_event(OperationEvent::Admit);
    assert_eq!(again.unwrap_err(), DecideError::OperationClosed);
}

#[test]
fn admit_creates_a_wall_time_wake_only_with_a_wall_limit() {
    let mut limited = Fixture::new(BudgetLimits {
        wall_seconds: Some(3600),
        ..no_limits()
    });
    let decision = limited.event(OperationEvent::Admit);
    let [wake] = decision.effects.as_slice() else {
        panic!("expected one effect, got {:?}", decision.effects);
    };
    assert_eq!(wake.kind, EffectKind::Wake);
    assert_eq!(wake.payload["purpose"], "wall_time");
    assert_eq!(wake.payload["v"], 1);
    assert_eq!(
        wake.not_before,
        limited.now + SignedDuration::from_secs(3600)
    );
    assert!(wake.max_attempts >= 1 && wake.dedup_key.is_some());

    let mut unlimited = Fixture::new(no_limits());
    assert!(unlimited.event(OperationEvent::Admit).effects.is_empty());
}

#[test]
fn propose_refuses_kinds_the_writer_owns() {
    let mut fx = Fixture::running(no_limits());
    let run = fx.proposal(fx.intent(&[]));
    let writer_owned = [
        StepIntent::Execution(run.intent.clone()),
        StepIntent::Approval {
            execution: run.step,
            plan_hash: hash(1),
            expires_at: run.expires_at,
        },
        StepIntent::Verification {
            execution: run.step,
            delay_secs: 30,
        },
        StepIntent::PolicyDecision {
            execution: run.step,
        },
    ];
    for intent in writer_owned {
        let kind = intent.kind();
        let step = fx.new_step(intent);
        let result = fx.try_event(OperationEvent::Propose {
            steps: vec![step],
            edges: Vec::new(),
        });
        assert_eq!(result.unwrap_err(), DecideError::KindNotProposable(kind));
    }
    assert!(fx.state.snapshot().steps.is_empty());
}

#[test]
fn propose_assigns_cycle_and_dense_ordinals() {
    let mut fx = Fixture::running(no_limits());
    fx.tweak(|s| s.operation.cycle = 2);
    let batch = vec![fx.note(), fx.plan(), fx.note()];
    let first = fx.event(OperationEvent::Propose {
        steps: batch,
        edges: Vec::new(),
    });
    let ordinals: Vec<u32> = first.steps.iter().map(|r| r.step.ordinal).collect();
    assert_eq!(ordinals, [0, 1, 2]);
    let more = vec![fx.plan(), fx.note()];
    let second = fx.event(OperationEvent::Propose {
        steps: more,
        edges: Vec::new(),
    });
    let ordinals: Vec<u32> = second.steps.iter().map(|r| r.step.ordinal).collect();
    assert_eq!(ordinals, [3, 4]);
    assert!(
        first
            .steps
            .iter()
            .chain(&second.steps)
            .all(|r| r.step.cycle == 2)
    );
}

#[test]
fn propose_births_each_kind_in_its_outcome() {
    let mut fx = Fixture::running(no_limits());
    let turn = fx.new_step(StepIntent::ReasoningTurn {
        context_refs: Vec::new(),
    });
    let notification = fx.new_step(StepIntent::Notification {
        channel: "discord".to_owned(),
        subject: "plex is down".to_owned(),
    });
    let steps = vec![
        turn,
        fx.evidence(),
        fx.question(3600),
        notification,
        fx.new_step(StepIntent::Assessment {
            text: "the container exited".to_owned(),
        }),
        fx.plan(),
        fx.note(),
    ];
    let decision = fx.event(OperationEvent::Propose {
        steps,
        edges: Vec::new(),
    });
    let born: Vec<(StepKind, StepOutcome)> = decision
        .steps
        .iter()
        .map(|r| (r.step.kind, r.progress.outcome))
        .collect();
    assert_eq!(
        born,
        [
            (StepKind::ReasoningTurn, StepOutcome::Running),
            (StepKind::EvidenceRequest, StepOutcome::Pending),
            (StepKind::Question, StepOutcome::Pending),
            (StepKind::Notification, StepOutcome::Pending),
            (StepKind::Assessment, StepOutcome::Succeeded),
            (StepKind::Plan, StepOutcome::Succeeded),
            (StepKind::Note, StepOutcome::Succeeded),
        ]
    );
    for record in &decision.steps {
        let p = &record.progress;
        match record.progress.outcome {
            StepOutcome::Running => assert_eq!(p.started_at, Some(fx.now)),
            StepOutcome::Succeeded => {
                assert_eq!((p.started_at, p.ended_at), (Some(fx.now), Some(fx.now)));
            }
            _ => assert_eq!((p.started_at, p.ended_at), (None, None)),
        }
    }
    assert!(
        decision.progress.is_empty(),
        "born progress rides on the step"
    );
}

#[test]
fn edges_must_point_to_committed_steps() {
    let mut fx = Fixture::running(no_limits());
    let old = fx.propose_plan();
    let new = fx.note();
    let new_id = new.id;
    let missing = fx.fresh_step();
    let bad = |from, to| NewEdge {
        from,
        to,
        kind: EdgeKind::DependsOn,
    };
    let steps = |new: &NewStep| vec![new.clone()];
    for edge in [
        bad(new_id, missing),
        bad(missing, new_id),
        bad(old, new_id),
        bad(new_id, new_id),
    ] {
        let result = fx.try_event(OperationEvent::Propose {
            steps: steps(&new),
            edges: vec![edge],
        });
        assert!(matches!(result, Err(DecideError::BadEdge(_))), "{edge:?}");
    }
    assert_eq!(fx.state.step_count(), 1, "a refused batch changes nothing");
    let ok = fx.event(OperationEvent::Propose {
        steps: steps(&new),
        edges: vec![bad(new_id, old)],
    });
    assert_eq!(ok.edges.len(), 1);
}

#[test]
fn edge_kind_endpoint_rules_are_enforced() {
    let mut fx = Fixture::running(no_limits());
    let earlier = fx.propose_two();
    let later = fx.note();
    let later_id = later.id;
    for kind in [EdgeKind::Answers, EdgeKind::Verifies, EdgeKind::RetryOf] {
        let result = fx.try_event(OperationEvent::Propose {
            steps: vec![later.clone()],
            edges: vec![NewEdge {
                from: later_id,
                to: earlier,
                kind,
            }],
        });
        assert!(matches!(result, Err(DecideError::BadEdge(_))), "{kind:?}");
    }
    for kind in [EdgeKind::DependsOn, EdgeKind::CausedBy] {
        let mut fresh = Fixture::running(no_limits());
        let before = fresh.propose_plan();
        let step = fresh.note();
        let from = step.id;
        let result = fresh.try_event(OperationEvent::Propose {
            steps: vec![step],
            edges: vec![NewEdge {
                from,
                to: before,
                kind,
            }],
        });
        assert!(result.is_ok(), "{kind:?}");
    }
}

#[test]
fn evidence_request_creates_a_collect_effect_and_counts_the_budget() {
    let mut fx = Fixture::running(no_limits());
    let step = fx.evidence();
    let id = step.id;
    let decision = fx.event(OperationEvent::Propose {
        steps: vec![step],
        edges: Vec::new(),
    });
    let [effect] = decision.effects.as_slice() else {
        panic!("{:?}", decision.effects);
    };
    assert_eq!(effect.kind, EffectKind::CollectEvidence);
    assert_eq!(effect.max_attempts, 3);
    assert_eq!(effect.not_before, fx.now);
    assert_eq!(
        effect.dedup_key.as_deref(),
        Some(format!("collect:{id}").as_str())
    );
    assert_eq!(effect.payload["step"], id.to_string());
    assert_eq!(effect.payload["v"], 1);
    assert_eq!(decision.operation.budgets.used.evidence_requests, 1);
}

#[test]
fn evidence_budget_spent_stores_a_refused_step_and_pauses() {
    let mut fx = Fixture::running(BudgetLimits {
        evidence_requests: Some(1),
        ..no_limits()
    });
    let first = fx.propose_evidence();
    assert_eq!(fx.outcome(first), StepOutcome::Pending);
    let step = fx.evidence();
    let id = step.id;
    let decision = fx.event(OperationEvent::Propose {
        steps: vec![step],
        edges: Vec::new(),
    });
    let record = &decision.steps[0];
    assert_eq!(record.progress.outcome, StepOutcome::Refused);
    assert_eq!(
        record.progress.reason.as_deref(),
        Some("evidence budget spent")
    );
    assert!(decision.effects.is_empty());
    assert_eq!(decision.operation.budgets.used.evidence_requests, 1);
    assert_eq!(
        fx.status(),
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::EvidenceRequests
            }
        }
    );
    assert_eq!(fx.outcome(id), StepOutcome::Refused);
}

#[test]
fn question_creates_a_timeout_wake() {
    let mut fx = Fixture::running(no_limits());
    let question = fx.question(7200);
    let id = question.id;
    let decision = fx.event(OperationEvent::Propose {
        steps: vec![question],
        edges: Vec::new(),
    });
    let [wake] = decision.effects.as_slice() else {
        panic!("{:?}", decision.effects);
    };
    assert_eq!(wake.kind, EffectKind::Wake);
    assert_eq!(wake.not_before, fx.now + SignedDuration::from_secs(7200));
    assert_eq!(wake.payload["purpose"], "question_timeout");
    assert_eq!(wake.payload["step"], id.to_string());
    assert_eq!(fx.status(), OperationStatus::WaitingUser);
}

#[test]
fn answer_creates_a_note_step_with_an_answers_edge_and_finishes_the_question() {
    let mut fx = Fixture::running(no_limits());
    let question = fx.propose_question(3600);
    let note = fx.note();
    let note_id = note.id;
    let decision = fx.event(OperationEvent::Answer { question, note });
    assert_eq!(decision.steps.len(), 1);
    assert_eq!(decision.steps[0].progress.outcome, StepOutcome::Succeeded);
    let [edge] = decision.edges.as_slice() else {
        panic!("{:?}", decision.edges);
    };
    assert_eq!(
        (edge.from, edge.to, edge.kind),
        (note_id, question, EdgeKind::Answers)
    );
    assert_eq!(fx.outcome(question), StepOutcome::Succeeded);
    assert_eq!(fx.status(), OperationStatus::Running);
    let again = fx.note();
    let result = fx.try_event(OperationEvent::Answer {
        question,
        note: again,
    });
    assert_eq!(result.unwrap_err(), DecideError::StepNotOpen(question));
}

#[test]
fn budget_exhaustion_pauses() {
    let mut fx = Fixture::running(BudgetLimits {
        executions: Some(1),
        tokens: Some(100),
        model_turns: Some(1),
        ..no_limits()
    });
    fx.propose_run();
    let intent = fx.intent(&[("signal", "KILL")]);
    let proposal = fx.proposal(intent);
    let id = proposal.step;
    let decision = fx.event(OperationEvent::ProposeExecution(proposal));
    assert_eq!(
        decision.steps.len(),
        1,
        "no approval step for a refused run"
    );
    assert!(decision.executions.is_empty());
    assert_eq!(
        decision.steps[0].progress.reason.as_deref(),
        Some("execution budget spent")
    );
    assert_eq!(fx.outcome(id), StepOutcome::Refused);
    assert_eq!(
        fx.status(),
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::Executions
            }
        }
    );

    let mut tokens = Fixture::running(BudgetLimits {
        tokens: Some(100),
        ..no_limits()
    });
    let turn = tokens.new_step(StepIntent::ReasoningTurn {
        context_refs: Vec::new(),
    });
    let turn_id = turn.id;
    tokens.propose(vec![turn]);
    let usage = StepUsage {
        input_tokens: 80,
        output_tokens: 20,
        cost_micros: None,
    };
    let finish = |usage| OperationEvent::StepFinished {
        step: turn_id,
        outcome: StepOutcome::Succeeded,
        summary: None,
        reason: None,
        output: None,
        usage: Some(usage),
    };
    tokens.event(finish(usage));
    assert_eq!(tokens.state.operation().budgets.used.tokens, 100);
    assert_eq!(
        tokens.status(),
        OperationStatus::Running,
        "exactly the limit is allowed"
    );
    let mut over = Fixture::running(BudgetLimits {
        tokens: Some(100),
        ..no_limits()
    });
    let turn = over.new_step(StepIntent::ReasoningTurn {
        context_refs: Vec::new(),
    });
    let over_id = turn.id;
    over.propose(vec![turn]);
    over.event(OperationEvent::StepFinished {
        step: over_id,
        outcome: StepOutcome::Succeeded,
        summary: None,
        reason: None,
        output: None,
        usage: Some(StepUsage {
            input_tokens: 80,
            output_tokens: 21,
            cost_micros: None,
        }),
    });
    assert_eq!(
        over.status(),
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::Tokens
            }
        }
    );
}

fn paused_for_evidence() -> Fixture {
    let mut fx = Fixture::running(BudgetLimits {
        evidence_requests: Some(1),
        ..no_limits()
    });
    fx.propose_evidence();
    fx.propose_evidence();
    assert!(matches!(fx.status(), OperationStatus::Paused { .. }));
    fx
}

#[test]
fn resume_needs_an_extended_budget() {
    let mut fx = paused_for_evidence();
    let result = fx.try_event(OperationEvent::Resume { extend: Vec::new() });
    assert_eq!(
        result.unwrap_err(),
        DecideError::BudgetStillSpent(BudgetKind::EvidenceRequests)
    );
    let result = fx.try_event(OperationEvent::Resume {
        extend: vec![(BudgetKind::EvidenceRequests, Some(1))],
    });
    assert!(result.is_err(), "a limit equal to the use is still spent");
    assert!(matches!(fx.status(), OperationStatus::Paused { .. }));
}

#[test]
fn resume_with_an_extended_budget_returns_to_running() {
    let mut fx = paused_for_evidence();
    let decision = fx.event(OperationEvent::Resume {
        extend: vec![(BudgetKind::EvidenceRequests, Some(5))],
    });
    assert_eq!(fx.status(), OperationStatus::Running);
    assert_eq!(decision.operation.budgets.limits.evidence_requests, Some(5));
    assert!(
        decision
            .audit
            .iter()
            .any(|a| a.kind == domovoi_engine::operations::audit::OPERATION_RESUMED)
    );
    let more = fx.propose_evidence();
    assert_eq!(fx.outcome(more), StepOutcome::Pending);
}

#[test]
fn paused_operation_refuses_new_steps() {
    let mut fx = paused_for_evidence();
    let step = fx.note();
    let result = fx.try_event(OperationEvent::Propose {
        steps: vec![step],
        edges: Vec::new(),
    });
    assert_eq!(result.unwrap_err(), DecideError::Paused);
    let intent = fx.intent(&[]);
    let proposal = fx.proposal(intent);
    let result = fx.try_event(OperationEvent::ProposeExecution(proposal));
    assert_eq!(result.unwrap_err(), DecideError::Paused);
    let pause = fx.try_event(OperationEvent::Pause {
        reason: PauseReason::GlobalPause,
    });
    assert_eq!(pause.unwrap_err(), DecideError::Paused);
}
