//! `decide` for executions: proposing, deciding, dispatching, settling, verifying and the
//! events that end or measure the operation.

use domovoi_core::ports::{EffectKind, JournalKind};
use domovoi_core::{
    BudgetLimits, EdgeKind, ExecutionId, OperationStatus, PauseReason, RetryOf, StepIntent,
    StepKind, StepOutcome,
};
use domovoi_engine::operations::audit;
use domovoi_engine::operations::{DecideError, OperationEvent};
use jiff::SignedDuration;

use crate::support::Fixture;

fn no_limits() -> BudgetLimits {
    BudgetLimits::default()
}

fn minute() -> Option<SignedDuration> {
    Some(SignedDuration::from_secs(60))
}

#[test]
fn execution_proposal_creates_approval_then_execution_with_depends_on_and_a_row() {
    let mut fx = Fixture::running(no_limits());
    let cause = fx.propose_plan();
    let intent = fx.intent(&[("timeout", "10")]);
    let mut proposal = fx.proposal(intent);
    proposal.caused_by = Some(cause);
    let (exec, approval, expires) = (proposal.step, proposal.approval_step, proposal.expires_at);
    let decision = fx.event(OperationEvent::ProposeExecution(proposal));

    let kinds: Vec<_> = decision
        .steps
        .iter()
        .map(|r| (r.step.id, r.step.kind))
        .collect();
    assert_eq!(
        kinds,
        [(approval, StepKind::Approval), (exec, StepKind::Execution)]
    );
    assert!(decision.steps[0].step.ordinal < decision.steps[1].step.ordinal);
    assert!(
        decision
            .steps
            .iter()
            .all(|r| r.progress.outcome == StepOutcome::Pending)
    );
    let edges: Vec<_> = decision
        .edges
        .iter()
        .map(|e| (e.from, e.to, e.kind))
        .collect();
    assert_eq!(
        edges,
        [
            (exec, approval, EdgeKind::DependsOn),
            (exec, cause, EdgeKind::CausedBy)
        ]
    );
    let [row] = decision.executions.as_slice() else {
        panic!("{:?}", decision.executions);
    };
    assert_eq!(row.execution, ExecutionId::from_step(exec));
    assert!(row.approval().is_none());
    let [wake] = decision.effects.as_slice() else {
        panic!("{:?}", decision.effects);
    };
    assert_eq!(wake.kind, EffectKind::Wake);
    assert_eq!(wake.not_before, expires);
    assert_eq!(
        wake.dedup_key.as_deref(),
        Some(format!("wake:expiry:{exec}").as_str())
    );
    assert_eq!(wake.payload["purpose"], "approval_expiry");
    assert_eq!(decision.audit[0].kind, audit::EXECUTION_PROPOSED);
    assert_eq!(decision.audit[0].detail["step"], exec.to_string());
    assert_eq!(decision.operation.budgets.used.executions, 1);
    assert_eq!(fx.status(), OperationStatus::WaitingApproval);
}

#[test]
fn execution_proposal_for_an_unpinned_target_is_stored_refused() {
    let mut fx = Fixture::running(no_limits());
    let mut intent = fx.intent(&[]);
    intent.target = Some(domovoi_core::TargetKey::parse("jellyfin").unwrap());
    let proposal = fx.proposal(intent);
    let exec = proposal.step;
    let decision = fx.event(OperationEvent::ProposeExecution(proposal));
    let [record] = decision.steps.as_slice() else {
        panic!("{:?}", decision.steps);
    };
    assert_eq!(record.step.id, exec);
    assert_eq!(record.progress.outcome, StepOutcome::Refused);
    assert_eq!(
        record.progress.reason.as_deref(),
        Some("target is not pinned to this operation")
    );
    assert!(decision.executions.is_empty() && decision.effects.is_empty());
    assert!(decision.edges.is_empty());
    assert_eq!(decision.audit[0].kind, audit::EXECUTION_REFUSED);
    assert_eq!(decision.operation.budgets.used.executions, 0);

    let mut named = fx.proposal(fx.intent(&[]));
    named.refused = Some("unknown skill".to_owned());
    named.plan_hash = None;
    let decision = fx.event(OperationEvent::ProposeExecution(named));
    assert_eq!(
        decision.steps[0].progress.reason.as_deref(),
        Some("unknown skill")
    );
}

/// A run that ended as `outcome`, then (for an unknown outcome) a resumed operation.
fn run_that_ended(outcome: StepOutcome) -> (Fixture, domovoi_core::StepId) {
    let mut fx = Fixture::running(no_limits());
    let step = fx.dispatched_run();
    fx.settle(step, outcome, None).unwrap();
    if outcome == StepOutcome::OutcomeUnknown {
        fx.event(OperationEvent::Resume { extend: Vec::new() });
    }
    (fx, step)
}

#[test]
fn no_blind_retry_of_failed_step() {
    for outcome in [StepOutcome::Failed, StepOutcome::OutcomeUnknown] {
        let (mut fx, _) = run_that_ended(outcome);
        let proposal = fx.proposal(fx.intent(&[]));
        let again = proposal.step;
        let decision = fx.event(OperationEvent::ProposeExecution(proposal));
        assert_eq!(decision.steps.len(), 1, "{outcome:?}");
        assert_eq!(
            decision.steps[0].progress.reason.as_deref(),
            Some("the same run failed earlier; say what changed")
        );
        assert_eq!(fx.outcome(again), StepOutcome::Refused);
        let mut blank = fx.intent(&[]);
        blank.retry_of = Some(RetryOf {
            step: decision.steps[0].step.id,
            changed: "  ".to_owned(),
        });
        let proposal = fx.proposal(blank);
        let blank_id = proposal.step;
        fx.event(OperationEvent::ProposeExecution(proposal));
        assert_eq!(
            fx.outcome(blank_id),
            StepOutcome::Refused,
            "a blank change states nothing"
        );
    }
}

#[test]
fn retry_with_a_stated_change_is_linked_with_retry_of() {
    let (mut fx, failed) = run_that_ended(StepOutcome::Failed);
    let mut intent = fx.intent(&[]);
    intent.retry_of = Some(RetryOf {
        step: failed,
        changed: "the disk has room now".to_owned(),
    });
    let proposal = fx.proposal(intent);
    let exec = proposal.step;
    let decision = fx.event(OperationEvent::ProposeExecution(proposal));
    assert_eq!(decision.steps.len(), 2);
    assert!(
        decision
            .edges
            .iter()
            .any(|e| (e.from, e.to, e.kind) == (exec, failed, EdgeKind::RetryOf))
    );
    assert_eq!(fx.outcome(exec), StepOutcome::Pending);
}

#[test]
fn authorization_creates_one_dispatch_effect_and_finishes_the_approval() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.propose_run();
    let unapproved = fx.try_event(OperationEvent::ExecutionAuthorized { step: exec });
    assert!(
        matches!(unapproved, Err(DecideError::Invalid(_))),
        "no approval on the row"
    );
    let decision = fx.authorize_run(exec);
    let approval = fx
        .state
        .snapshot()
        .steps
        .iter()
        .find(|r| r.step.kind == StepKind::Approval);
    assert_eq!(approval.unwrap().progress.outcome, StepOutcome::Succeeded);
    let [dispatch] = decision.effects.as_slice() else {
        panic!("{:?}", decision.effects);
    };
    assert_eq!(dispatch.kind, EffectKind::Dispatch);
    assert_eq!(dispatch.max_attempts, 5);
    assert_eq!(dispatch.not_before, fx.now);
    assert_eq!(
        dispatch.dedup_key.as_deref(),
        Some(format!("dispatch:{exec}").as_str())
    );
    let audited = &decision.audit[0];
    assert_eq!(audited.kind, audit::EXECUTION_APPROVED);
    assert_eq!(audited.detail["plan_hash"].as_str().unwrap().len(), 8);
    assert_eq!(audited.detail["client"], "test");
    assert!(
        decision
            .journal
            .iter()
            .any(|n| n.kind == JournalKind::ExecutionTransition && n.payload["to"] == "authorized")
    );
    assert_eq!(fx.status(), OperationStatus::Running);
    let twice = fx.try_event(OperationEvent::ExecutionAuthorized { step: exec });
    assert_eq!(twice.unwrap_err(), DecideError::StepNotOpen(exec));
}

#[test]
fn denial_refuses_approval_and_execution() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.propose_run();
    let decision = fx.event(OperationEvent::ExecutionDenied { step: exec });
    assert_eq!(fx.outcome(exec), StepOutcome::Refused);
    let reasons: Vec<_> = decision
        .progress
        .iter()
        .map(|c| c.to.reason.as_deref())
        .collect();
    assert_eq!(
        reasons,
        [Some("denied by a person"), Some("denied by a person")]
    );
    assert_eq!(decision.audit[0].kind, audit::EXECUTION_DENIED);
    assert!(decision.effects.is_empty());
    assert_eq!(fx.status(), OperationStatus::Running);
}

#[test]
fn expiry_marks_approval_and_execution_not_run() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.propose_run();
    fx.advance(25 * 3600);
    let decision = fx.event(OperationEvent::ExecutionExpired { step: exec });
    assert_eq!(fx.outcome(exec), StepOutcome::NotRun);
    let exec_change = decision.progress.iter().find(|c| c.step == exec).unwrap();
    assert_eq!(
        exec_change.to.reason.as_deref(),
        Some("the approval expired before the action was dispatched")
    );
    assert!(
        decision
            .progress
            .iter()
            .all(|c| c.to.outcome == StepOutcome::NotRun)
    );
    assert_eq!(decision.audit[0].kind, audit::EXECUTION_EXPIRED);
}

#[test]
fn settle_enforces_allowed_outcome_pairs() {
    let from_pending = [
        (StepOutcome::Refused, true),
        (StepOutcome::NotRun, true),
        (StepOutcome::Unavailable, true),
        (StepOutcome::Succeeded, false),
        (StepOutcome::Failed, false),
        (StepOutcome::OutcomeUnknown, false),
        (StepOutcome::Skipped, false),
        (StepOutcome::Running, false),
    ];
    for (outcome, allowed) in from_pending {
        let mut fx = Fixture::running(no_limits());
        let exec = fx.propose_run();
        fx.authorize_run(exec);
        let result = fx.settle(exec, outcome, None);
        assert_eq!(result.is_ok(), allowed, "pending to {outcome:?}");
        if !allowed {
            assert!(matches!(result, Err(DecideError::Invalid(_))));
        }
    }
    let from_running = [
        (StepOutcome::Succeeded, true),
        (StepOutcome::Failed, true),
        (StepOutcome::OutcomeUnknown, true),
        (StepOutcome::NotRun, true),
        (StepOutcome::Refused, false),
        (StepOutcome::Unavailable, false),
        (StepOutcome::Cancelled, false),
    ];
    for (outcome, allowed) in from_running {
        let mut fx = Fixture::running(no_limits());
        let exec = fx.dispatched_run();
        let result = fx.settle(exec, outcome, None);
        assert_eq!(result.is_ok(), allowed, "running to {outcome:?}");
    }
    let (mut fx, exec) = run_that_ended(StepOutcome::Failed);
    let again = fx.settle(exec, StepOutcome::Succeeded, None);
    assert_eq!(again.unwrap_err(), DecideError::StepNotOpen(exec));
}

#[test]
fn successful_settle_creates_verification_with_verifies_edge_and_a_delayed_verify_effect() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.dispatched_run();
    let decision = fx.settle(exec, StepOutcome::Succeeded, minute()).unwrap();
    let [verification] = decision.steps.as_slice() else {
        panic!("{:?}", decision.steps);
    };
    assert_eq!(verification.step.kind, StepKind::Verification);
    assert_eq!(verification.step.phase, "verify");
    assert_eq!(verification.progress.outcome, StepOutcome::Pending);
    assert!(matches!(
        verification.step.intent,
        StepIntent::Verification { execution, delay_secs: 60 } if execution == exec
    ));
    let [edge] = decision.edges.as_slice() else {
        panic!("{:?}", decision.edges);
    };
    assert_eq!(
        (edge.from, edge.to, edge.kind),
        (verification.step.id, exec, EdgeKind::Verifies)
    );
    let [verify] = decision.effects.as_slice() else {
        panic!("{:?}", decision.effects);
    };
    assert_eq!(verify.kind, EffectKind::Verify);
    assert_eq!(verify.not_before, fx.now + SignedDuration::from_secs(60));
    assert_eq!(
        verify.dedup_key.as_deref(),
        Some(format!("verify:{exec}").as_str())
    );
    assert_eq!(verify.payload["step"], verification.step.id.to_string());
    assert_eq!(verify.payload["execution"], exec.to_string());
    assert_eq!(decision.audit[0].kind, audit::EXECUTION_SUCCEEDED);
    assert_eq!(fx.status(), OperationStatus::WaitingTimer);

    let mut plain = Fixture::running(no_limits());
    let exec = plain.dispatched_run();
    let decision = plain.settle(exec, StepOutcome::Failed, minute()).unwrap();
    assert!(decision.steps.is_empty() && decision.effects.is_empty());
}

#[test]
fn outcome_unknown_pauses_the_operation() {
    let mut fx = Fixture::running(no_limits());
    let exec = fx.dispatched_run();
    let decision = fx
        .settle(exec, StepOutcome::OutcomeUnknown, minute())
        .unwrap();
    assert_eq!(
        fx.status(),
        OperationStatus::Paused {
            reason: PauseReason::OutcomeUnknown { step: exec }
        }
    );
    assert!(
        decision.steps.is_empty(),
        "no verification after an unknown outcome"
    );
    let kinds: Vec<_> = decision.audit.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [audit::EXECUTION_OUTCOME_UNKNOWN, audit::OPERATION_PAUSED]
    );
}
