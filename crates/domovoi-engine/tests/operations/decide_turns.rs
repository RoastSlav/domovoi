//! Model turns are charged when they are accepted, and a question stays open from the moment
//! it is proposed until it is answered or times out, started or not.

use domovoi_core::{
    BudgetKind, BudgetLimits, OperationStatus, PauseReason, StepIntent, StepOutcome, StepUsage,
    WaitKind,
};
use domovoi_engine::operations::OperationEvent;

use crate::support::Fixture;

fn turns(limit: u32) -> BudgetLimits {
    BudgetLimits {
        model_turns: Some(limit),
        tokens: Some(1000),
        cost_micros: Some(5000),
        ..BudgetLimits::default()
    }
}

fn turn(fx: &mut Fixture) -> domovoi_engine::operations::NewStep {
    fx.new_step(StepIntent::ReasoningTurn {
        context_refs: Vec::new(),
    })
}

fn finish(step: domovoi_core::StepId, usage: Option<StepUsage>) -> OperationEvent {
    OperationEvent::StepFinished {
        step,
        outcome: StepOutcome::Succeeded,
        summary: None,
        reason: None,
        output: None,
        usage,
    }
}

#[test]
fn a_turn_is_charged_when_accepted_even_if_it_reports_no_usage() {
    let mut fx = Fixture::running(turns(3));
    let step = turn(&mut fx);
    let id = step.id;
    fx.propose(vec![step]);
    assert_eq!(fx.state.operation().budgets.used.model_turns, 1);

    fx.event(finish(id, None));
    let used = &fx.state.operation().budgets.used;
    assert_eq!(
        used.model_turns, 1,
        "finishing without usage charges nothing more"
    );
    assert_eq!((used.tokens, used.cost_micros), (0, 0));
}

#[test]
fn finishing_a_turn_charges_tokens_and_cost_but_not_the_turn_again() {
    let mut fx = Fixture::running(turns(3));
    let step = turn(&mut fx);
    let id = step.id;
    fx.propose(vec![step]);
    let usage = StepUsage {
        input_tokens: 60,
        output_tokens: 40,
        cost_micros: Some(700),
    };
    fx.event(finish(id, Some(usage)));
    let used = &fx.state.operation().budgets.used;
    assert_eq!(used.model_turns, 1);
    assert_eq!(used.tokens, 100);
    assert_eq!(used.cost_micros, 700);
}

#[test]
fn a_batch_of_turns_stops_at_the_limit_and_refused_turns_are_not_charged() {
    let mut fx = Fixture::running(turns(2));
    let steps: Vec<_> = (0..4).map(|_| turn(&mut fx)).collect();
    let ids = fx.propose(steps);
    let outcomes: Vec<_> = ids.iter().map(|id| fx.outcome(*id)).collect();
    assert_eq!(
        outcomes,
        vec![
            StepOutcome::Running,
            StepOutcome::Running,
            StepOutcome::Refused,
            StepOutcome::Refused
        ]
    );
    assert_eq!(fx.state.operation().budgets.used.model_turns, 2);
    assert_eq!(
        fx.status(),
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::ModelTurns
            }
        }
    );
}

#[test]
fn a_started_question_is_still_waiting_and_can_be_answered() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let question = fx.propose_question(3600);
    assert_eq!(fx.state.snapshot().waits(), vec![WaitKind::User]);

    fx.event(OperationEvent::StepStarted { step: question });
    assert_eq!(fx.outcome(question), StepOutcome::Running);
    assert_eq!(fx.state.snapshot().waits(), vec![WaitKind::User]);
    assert_eq!(fx.status(), OperationStatus::WaitingUser);

    let note = fx.note();
    fx.event(OperationEvent::Answer { question, note });
    assert_eq!(fx.outcome(question), StepOutcome::Succeeded);
    assert!(fx.state.snapshot().waits().is_empty());
    assert_eq!(fx.status(), OperationStatus::Running);
}

#[test]
fn a_started_question_that_times_out_stops_waiting() {
    let mut fx = Fixture::running(BudgetLimits::default());
    let question = fx.propose_question(3600);
    fx.event(OperationEvent::StepStarted { step: question });
    assert_eq!(fx.status(), OperationStatus::WaitingUser);

    fx.event(OperationEvent::StepFinished {
        step: question,
        outcome: StepOutcome::NotRun,
        summary: None,
        reason: Some("no answer before the timeout".to_owned()),
        output: None,
        usage: None,
    });
    assert_eq!(fx.outcome(question), StepOutcome::NotRun);
    assert!(fx.state.snapshot().waits().is_empty());
    assert_eq!(fx.status(), OperationStatus::Running);
}
