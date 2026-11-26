//! What a hub restart leaves behind and how the new service finishes it: approved work still
//! goes out, and a run whose answer was lost is settled from the agent's journal.

use domovoi_core::ports::{ActionAnswer, LinkError};
use domovoi_core::{
    Actor, ExecutionId, OperationId, OperationStatus, PauseReason, StepId, StepKind, StepOutcome,
};

use crate::drive::new_epoch;
use crate::harness::{World, host_id, world, world_gated};

/// A run that went out and whose answer never came back, so it stays dispatched.
async fn dispatched_without_an_answer(w: &World) -> (OperationId, StepId) {
    w.link
        .on_skill(|_, _| Err(LinkError::Indeterminate("link dropped".to_owned())));
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Running).await;
    (op, step)
}

#[tokio::test]
async fn operation_resumes_after_restart() {
    let mut w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    assert!(w.skill_requests().is_empty(), "approved, not yet sent");

    w.restart().await;
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    assert_eq!(w.skill_requests().len(), 1, "sent once, by the new service");
}

#[tokio::test]
async fn reconcile_settles_a_dispatched_execution_after_restart() {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    w.link.answer(
        host_id(),
        ExecutionId::from_step(step),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );

    w.restart().await;
    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    assert_eq!(w.skill_requests().len(), 1, "it was not sent again");
    let audit = w.audit_kinds().await;
    assert!(audit.contains(&"execution.succeeded".to_owned()));
}

#[tokio::test]
async fn reconcile_with_a_changed_epoch_settles_outcome_unknown_and_pauses() {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    // The agent came back with a fresh journal: nothing it says can be tied to this run.
    w.link.set_journal_epoch(host_id(), new_epoch());

    w.restart().await;
    let snapshot = w
        .until(op, |s| {
            s.step(step)
                .is_some_and(|r| r.progress.outcome == StepOutcome::OutcomeUnknown)
        })
        .await;

    assert_eq!(
        snapshot.operation.status,
        OperationStatus::Paused {
            reason: PauseReason::OutcomeUnknown { step }
        }
    );
    let reason = snapshot.step(step).unwrap().progress.reason.clone();
    assert!(reason.unwrap_or_default().contains("journal was reset"));
    assert_eq!(w.skill_requests().len(), 1, "nothing was sent again");
}

#[tokio::test]
async fn reconcile_not_found_settles_not_run() {
    let mut w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    // The journal has no record of the request; the fake answers `NotFound` unprompted.

    w.restart().await;
    let snapshot = w.until_outcome(op, step, StepOutcome::NotRun).await;

    let reason = snapshot.step(step).unwrap().progress.reason.clone();
    assert!(
        reason
            .unwrap_or_default()
            .contains("the agent has no record of this run")
    );
    assert!(
        snapshot
            .steps
            .iter()
            .all(|record| record.step.kind != StepKind::Verification)
    );
}

#[tokio::test]
async fn result_after_cancel_is_recorded() {
    let w = world().await;
    let (op, step) = dispatched_without_an_answer(&w).await;
    w.queue.close();
    w.settle().await;
    w.handle()
        .cancel(op, Actor::User(w.user), "changed my mind".to_owned())
        .await
        .unwrap();
    let cancelled = w.load(op).await;
    assert_eq!(cancelled.operation.status, OperationStatus::Cancelled);
    assert_eq!(
        cancelled.step(step).unwrap().progress.outcome,
        StepOutcome::Running,
        "a run that went out is not recalled"
    );

    // The result arrives anyway: the journal says it worked.
    w.link.answer(
        host_id(),
        ExecutionId::from_step(step),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    w.queue.open();
    let snapshot = w.until_outcome(op, step, StepOutcome::Succeeded).await;

    assert_eq!(snapshot.operation.status, OperationStatus::Cancelled);
    assert!(
        snapshot
            .steps
            .iter()
            .all(|record| record.step.kind != StepKind::Verification),
        "a late result starts nothing new"
    );
    assert!(
        w.audit_kinds()
            .await
            .contains(&"execution.succeeded".to_owned())
    );
}
