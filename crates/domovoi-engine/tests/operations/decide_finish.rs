//! `decide` for verification, cycles and the events that end, pause or measure the
//! operation.

use domovoi_core::ports::{EffectKind, JournalKind};
use domovoi_core::{
    BudgetKind, BudgetLimits, FinishOutcome, OperationStatus, PauseReason, StepIntent, StepKind,
    StepOutcome,
};
use domovoi_engine::operations::audit;
use domovoi_engine::operations::{DecideError, OperationEvent, derive_status};
use jiff::SignedDuration;

use crate::support::Fixture;

fn no_limits() -> BudgetLimits {
    BudgetLimits::default()
}

fn minute() -> Option<SignedDuration> {
    Some(SignedDuration::from_secs(60))
}

fn verifying() -> (Fixture, domovoi_core::StepId) {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.dispatched_run();
    fx.settle(exec, StepOutcome::Succeeded, minute()).unwrap();
    let verification = fx.verification_of(exec);
    (fx, verification)
}

fn verified(step: domovoi_core::StepId, passed: bool) -> OperationEvent {
    OperationEvent::Verified {
        step,
        passed,
        summary: "the container is up".to_owned(),
    }
}

#[test]
fn verification_passed_succeeds_the_operation_when_nothing_is_open() {
    let (mut fx, verification) = verifying();
    let decision = fx.event(verified(verification, true));
    assert_eq!(fx.status(), OperationStatus::Succeeded);
    assert_eq!(decision.operation.ended_at, Some(fx.now));
    assert_eq!(fx.outcome(verification), StepOutcome::Succeeded);
    assert!(decision.effects.is_empty());
}

#[test]
fn verification_passed_with_open_steps_keeps_running() {
    let (mut fx, verification) = verifying();
    let open = fx.propose_evidence();
    fx.event(verified(verification, true));
    assert_eq!(fx.status(), OperationStatus::Running);
    assert_eq!(fx.outcome(open), StepOutcome::Pending);
    assert_eq!(fx.state.operation().ended_at, None);
}

#[test]
fn failed_verification_starts_new_cycle() {
    let (mut fx, verification) = verifying();
    let decision = fx.event(verified(verification, false));
    assert_eq!(fx.state.operation().cycle, 1);
    assert_eq!(fx.status(), OperationStatus::Running);
    assert_eq!(fx.outcome(verification), StepOutcome::Failed);
    let [build] = decision.effects.as_slice() else {
        panic!("{:?}", decision.effects);
    };
    assert_eq!(build.kind, EffectKind::BuildContext);
    assert_eq!(build.payload["cycle"], 1);
    assert_eq!(build.payload["v"], 1);
    let operation = fx.state.operation().id;
    assert_eq!(
        build.dedup_key.as_deref(),
        Some(format!("build:{operation}:1").as_str())
    );
    assert!(
        decision
            .journal
            .iter()
            .any(|n| n.kind == JournalKind::OperationStatus && n.payload["cycle"] == 1)
    );
    let next = fx.propose_plan();
    assert_eq!(fx.state.step(next).unwrap().step.cycle, 1);
}

#[test]
fn cycle_limit_escalates() {
    // `is_spent(Cycles, cycle)` is asked with the cycle that just failed: a limit of 1 allows
    // one new cycle (0 to 1) and escalates when cycle 1 fails.
    let mut fx = Fixture::running(BudgetLimits {
        cycles: Some(1),
        ..no_limits()
    });
    let first = fx.dispatched_run();
    fx.settle(first, StepOutcome::Succeeded, minute()).unwrap();
    let verification = fx.verification_of(first);
    fx.event(verified(verification, false));
    assert_eq!(fx.status(), OperationStatus::Running);
    assert_eq!(fx.state.operation().cycle, 1);

    let intent = fx.intent(&[("signal", "TERM")]);
    let second = fx.propose_intent(intent);
    fx.authorize_run(second);
    fx.dispatch_run(second);
    fx.settle(second, StepOutcome::Succeeded, minute()).unwrap();
    let verification = fx.verification_of(second);
    let decision = fx.event(verified(verification, false));
    assert_eq!(fx.status(), OperationStatus::Escalated);
    assert_eq!(
        fx.state.operation().cycle,
        1,
        "an escalation starts no cycle"
    );
    assert!(decision.effects.is_empty());
    assert_eq!(decision.operation.ended_at, Some(fx.now));
}

#[test]
fn cancel_cancels_pending_steps_and_keeps_a_dispatched_execution_running() {
    let mut fx = Fixture::running(no_limits());
    let running = fx.dispatched_run();
    let question = fx.propose_question(3600);
    let evidence = fx.propose_evidence();
    let waiting = {
        let intent = fx.intent(&[("signal", "KILL")]);
        fx.propose_intent(intent)
    };
    let decision = fx.event(OperationEvent::Cancel {
        reason: "no longer needed".to_owned(),
    });
    assert_eq!(fx.status(), OperationStatus::Cancelled);
    assert_eq!(fx.outcome(running), StepOutcome::Running);
    for step in [question, evidence, waiting] {
        assert_eq!(fx.outcome(step), StepOutcome::Cancelled);
    }
    assert!(fx.state.open_steps().all(|r| r.step.id == running));
    assert_eq!(decision.operation.ended_at, Some(fx.now));
    assert!(
        decision
            .audit
            .iter()
            .any(|a| a.kind == audit::OPERATION_CANCELLED)
    );
    assert!(
        decision.effects.is_empty(),
        "the writer cancels the effects"
    );
}

#[test]
fn terminal_operation_refuses_new_steps_but_records_a_late_settle() {
    let mut fx = Fixture::running(no_limits());
    let running = fx.dispatched_run();
    fx.event(OperationEvent::Cancel {
        reason: "stop".to_owned(),
    });
    let step = fx.note();
    let closed = fx.try_event(OperationEvent::Propose {
        steps: vec![step],
        edges: Vec::new(),
    });
    assert_eq!(closed.unwrap_err(), DecideError::OperationClosed);
    assert_eq!(
        fx.try_event(OperationEvent::Tick).unwrap_err(),
        DecideError::OperationClosed
    );
    let decision = fx
        .settle(running, StepOutcome::Succeeded, minute())
        .unwrap();
    assert_eq!(fx.outcome(running), StepOutcome::Succeeded);
    assert!(decision.steps.is_empty() && decision.edges.is_empty() && decision.effects.is_empty());
    assert_eq!(fx.status(), OperationStatus::Cancelled);
    assert_eq!(decision.audit[0].kind, audit::EXECUTION_SUCCEEDED);
    let finished_twice = fx.settle(running, StepOutcome::Failed, None);
    assert_eq!(
        finished_twice.unwrap_err(),
        DecideError::StepNotOpen(running)
    );
}

#[test]
fn finish_skips_open_steps_and_refuses_while_an_execution_is_in_flight() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.dispatched_run();
    let result = fx.try_event(OperationEvent::Finish {
        outcome: FinishOutcome::Failed,
        summary: "giving up".to_owned(),
    });
    assert_eq!(result.unwrap_err(), DecideError::ExecutionInFlight(exec));
    fx.settle(exec, StepOutcome::Failed, None).unwrap();
    let question = fx.propose_question(3600);
    let turn = fx.new_step(StepIntent::ReasoningTurn {
        context_refs: Vec::new(),
    });
    let turn_id = turn.id;
    fx.propose(vec![turn]);
    let decision = fx.event(OperationEvent::Finish {
        outcome: FinishOutcome::Escalated,
        summary: "a person should look".to_owned(),
    });
    assert_eq!(fx.status(), OperationStatus::Escalated);
    assert_eq!(fx.outcome(question), StepOutcome::Skipped);
    assert_eq!(fx.outcome(turn_id), StepOutcome::Skipped);
    assert_eq!(decision.operation.ended_at, Some(fx.now));
    assert!(fx.state.open_steps().next().is_none());
}

#[test]
fn status_shows_the_highest_priority_wait() {
    let mut fx = Fixture::running(no_limits());
    fx.propose_question(3600);
    assert_eq!(fx.status(), OperationStatus::WaitingUser);
    let exec = fx.propose_run();
    assert_eq!(
        fx.status(),
        OperationStatus::WaitingApproval,
        "approval beats a question"
    );
    fx.authorize_run(exec);
    assert_eq!(fx.status(), OperationStatus::WaitingUser);
    fx.wait_for_agent(exec);
    assert_eq!(
        fx.status(),
        OperationStatus::WaitingUser,
        "a question beats an agent"
    );
    let question = fx
        .state
        .snapshot()
        .steps
        .iter()
        .find(|r| r.step.kind == StepKind::Question)
        .unwrap()
        .step
        .id;
    fx.event(OperationEvent::StepFinished {
        step: question,
        outcome: StepOutcome::NotRun,
        summary: None,
        reason: Some("no answer".to_owned()),
        output: None,
        usage: None,
    });
    assert_eq!(fx.status(), OperationStatus::WaitingAgent);
    fx.dispatch_run(exec);
    assert_eq!(fx.status(), OperationStatus::Running);
    fx.settle(exec, StepOutcome::Succeeded, minute()).unwrap();
    assert_eq!(fx.status(), OperationStatus::WaitingTimer);
    assert_eq!(
        derive_status(fx.state.snapshot()),
        OperationStatus::WaitingTimer
    );
}

#[test]
fn tick_pauses_on_wall_time_overrun() {
    let mut fx = Fixture::running(BudgetLimits {
        wall_seconds: Some(60),
        ..no_limits()
    });
    fx.advance(60);
    let decision = fx.event(OperationEvent::Tick);
    assert_eq!(decision.operation.budgets.used.wall_seconds, 60);
    assert_eq!(
        fx.status(),
        OperationStatus::Running,
        "exactly the limit is allowed"
    );
    fx.advance(1);
    fx.event(OperationEvent::Tick);
    assert_eq!(fx.state.operation().budgets.used.wall_seconds, 61);
    assert_eq!(
        fx.status(),
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::WallTime
            }
        }
    );
    let resumed = fx.try_event(OperationEvent::Resume { extend: Vec::new() });
    assert_eq!(
        resumed.unwrap_err(),
        DecideError::BudgetStillSpent(BudgetKind::WallTime)
    );
    let decision = fx.event(OperationEvent::Resume {
        extend: vec![(BudgetKind::WallTime, Some(3600))],
    });
    assert_eq!(fx.status(), OperationStatus::Running);
    assert!(
        decision
            .effects
            .iter()
            .any(|e| e.payload["purpose"] == "wall_time"),
        "a raised wall limit gets a new wake"
    );
}

#[test]
fn decide_does_not_mutate_its_input() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.dispatched_run();
    let before = fx.state.clone();
    let events = [
        OperationEvent::Cancel {
            reason: "stop".to_owned(),
        },
        OperationEvent::Tick,
        OperationEvent::Pause {
            reason: PauseReason::GlobalPause,
        },
    ];
    for event in events {
        let once = domovoi_engine::operations::decide::decide(
            &fx.state,
            event.clone(),
            fx.now,
            &mut crate::support::CounterIds::new(),
        );
        let twice = domovoi_engine::operations::decide::decide(
            &fx.state,
            event,
            fx.now,
            &mut crate::support::CounterIds::new(),
        );
        assert_eq!(once, twice, "the same inputs give the same decision");
        assert_eq!(fx.state, before);
    }
    let late = fx.settle(exec, StepOutcome::Succeeded, minute());
    assert!(late.is_ok());
}
