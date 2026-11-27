//! Investigations under the triage limits: the repeat cache, the hourly cap, the monthly
//! budget, failed runs, and waiting for an outcome.

use domovoi_core::ports::{EffectKind, EffectStatus, LlmError, UsageRepo};
use domovoi_core::{Incident, OperationId, OperationStatus, StepKind, StepOutcome};
use domovoi_engine::operations::InvestigateError;
use domovoi_engine::triage::TriageSettings;
use jiff::SignedDuration;

use crate::inv::{Inv, InvSetup, disk, inv, of_kind, output_kind, report, restart_report, turns};

fn hourly(runs: u32) -> InvSetup {
    InvSetup {
        triage: TriageSettings {
            per_hour_runs: runs,
            ..TriageSettings::default()
        },
        ..InvSetup::default()
    }
}

/// The turn that was skipped, once it is.
async fn until_skipped(w: &Inv, op: OperationId) -> domovoi_core::OperationSnapshot {
    w.until(op, |s| {
        turns(s)
            .iter()
            .any(|t| t.progress.outcome == StepOutcome::NotRun)
    })
    .await
}

/// The retry `build_context` effects of `op` that have not run, whatever their status.
async fn retries(w: &Inv, op: OperationId) -> Vec<domovoi_core::ports::EffectRecord> {
    w.all_effects(op, EffectKind::BuildContext)
        .await
        .into_iter()
        .filter(|effect| effect.payload["retry"] == true)
        .collect()
}

/// Runs a first investigation of another incident, so the hour's only slot is taken.
async fn take_the_slot(w: &Inv) -> Incident {
    let first = w.add_incident("redis", disk(50.0)).await;
    w.llm.push_text(report());
    let op = w.ops.investigate(&first, false).await.unwrap();
    w.until_closed(op).await;
    first
}

#[tokio::test]
async fn cached_report_goes_through_the_same_pipeline() {
    let w = inv(InvSetup::default()).await;
    w.llm.push_text(restart_report("nginx"));
    let first = w.start().await;
    w.until(first, |s| {
        s.operation.status == OperationStatus::WaitingApproval
    })
    .await;

    // The same check on the same target resolved earlier; a repeat opens with the same
    // evidence.
    w.resolve(&w.incident).await;
    let repeat = w.add_incident("nginx", disk(92.1)).await;
    let op = w.ops.investigate(&repeat, false).await.unwrap();
    let snapshot = w
        .until(op, |s| !of_kind(s, StepKind::Execution).is_empty())
        .await;

    let turn = turns(&snapshot)[0];
    assert_eq!(turn.progress.outcome, StepOutcome::Succeeded);
    assert_eq!(output_kind(turn), "cached");
    assert_eq!(w.llm.requests().len(), 1, "the model was not asked again");
    assert_eq!(of_kind(&snapshot, StepKind::Execution).len(), 1);
    assert_eq!(of_kind(&snapshot, StepKind::Approval).len(), 1);
    w.until(op, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;
    w.settle().await;
    assert_eq!(w.count(&repeat, "triage.cached").await, 1);
}

#[tokio::test]
async fn hourly_cap_skips_then_retries_when_the_hour_frees() {
    let w = inv(hourly(1)).await;
    take_the_slot(&w).await;
    let slot = w.now();
    let second = w.add_incident("a", disk(1.0)).await;
    let third = w.add_incident("b", disk(2.0)).await;
    let (b, c) = (
        w.ops.investigate(&second, false).await.unwrap(),
        w.ops.investigate(&third, false).await.unwrap(),
    );

    let snapshot = until_skipped(&w, b).await;
    until_skipped(&w, c).await;
    w.until(b, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;
    w.settle().await;
    assert_eq!(turns(&snapshot)[0].progress.outcome, StepOutcome::NotRun);
    assert_eq!(output_kind(turns(&snapshot)[0]), "skipped");
    assert_eq!(w.count(&second, "triage.skipped").await, 1);
    assert_eq!(
        w.llm.requests().len(),
        1,
        "no model call for a skipped turn"
    );
    let due = retries(&w, b).await;
    assert_eq!(due.len(), 1, "one retry is scheduled");
    let at = due[0].not_before;
    assert!(
        at > slot + SignedDuration::from_mins(58) && at <= slot + SignedDuration::from_mins(61),
        "the retry is due when the first run leaves the hour, not {at}"
    );

    // One slot frees: one retry runs, the other waits without a second skip.
    w.llm.push_text(report());
    w.llm.push_text(report());
    w.jump(61 * 60).await;
    w.until(b, |s| turns(s).len() >= 2).await;
    w.settle().await;
    let reports = w.count(&second, "triage.report").await + w.count(&third, "triage.report").await;
    let skips = w.count(&second, "triage.skipped").await + w.count(&third, "triage.skipped").await;
    assert_eq!(reports, 1, "one retry took the slot");
    assert_eq!(skips, 2, "the other retry waited without a new skip");
    assert_eq!(w.llm.requests().len(), 2);
}

#[tokio::test]
async fn monthly_budget_spent_skips_without_a_retry() {
    let w = inv(InvSetup {
        triage: TriageSettings {
            monthly_budget_micros: Some(1_000_000),
            ..TriageSettings::default()
        },
        ..InvSetup::default()
    })
    .await;
    UsageRepo::record(&*w.store, "anthropic", 10, 10, 1_000_000, w.now())
        .await
        .unwrap();
    let op = w.start().await;

    let snapshot = w.until_closed(op).await;

    assert_eq!(snapshot.operation.status, OperationStatus::Failed);
    assert!(w.llm.requests().is_empty());
    assert!(
        retries(&w, op).await.is_empty(),
        "a spent month is not retried"
    );
    assert_eq!(w.count(&w.incident, "triage.skipped").await, 1);
    assert_eq!(turns(&snapshot).len(), 1);
}

#[tokio::test]
async fn failed_run_retries_twice_then_stops() {
    let w = inv(hourly(10)).await;
    for _ in 0..3 {
        w.llm.push_err(LlmError::Unavailable("down".to_owned()));
    }
    let op = w.start().await;

    w.until_stepping(op, 300, |s| {
        turns(s)
            .iter()
            .filter(|t| t.progress.outcome == StepOutcome::Failed)
            .count()
            >= 2
    })
    .await;
    assert!(
        w.advisories().await.is_empty(),
        "two failures are not degraded"
    );
    let snapshot = w
        .until_stepping(op, 300, |s| s.operation.status.is_terminal())
        .await;

    assert_eq!(snapshot.operation.status, OperationStatus::Failed);
    assert_eq!(w.count(&w.incident, "triage.failed").await, 3);
    assert_eq!(
        retries(&w, op).await.len(),
        2,
        "exactly two retries were created"
    );
    assert_eq!(w.llm.requests().len(), 3);
    let open = w.advisories().await;
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].kind, domovoi_core::AdvisoryKind::TriageDegraded);
}

#[tokio::test]
async fn retry_after_resolution_ends_quietly() {
    let w = inv(hourly(1)).await;
    take_the_slot(&w).await;
    let op = w.start().await;
    until_skipped(&w, op).await;
    w.until(op, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;
    assert_eq!(w.llm.requests().len(), 1);

    w.resolve(&w.incident).await;
    w.llm.push_text(report());
    w.jump(61 * 60).await;
    let snapshot = w.until_closed(op).await;

    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(
        w.llm.requests().len(),
        1,
        "no model call after the incident is gone"
    );
    assert_eq!(w.count(&w.incident, "triage.skipped").await, 1);
    assert_eq!(w.count(&w.incident, "triage.report").await, 0);
}

#[tokio::test]
async fn investigate_and_wait_times_out_with_a_skipped_outcome() {
    // The model call is held back (a claimed effect's lease is shorter than the wait, so the
    // block is placed before the claim): the caller's wait ends first.
    let w = inv(InvSetup {
        held: vec![EffectKind::CallModel],
        ..InvSetup::default()
    })
    .await;
    w.llm.push_text(report());
    let waiting = w.spawn_wait();
    w.jump(10).await;
    let op = w.operations_of(&w.incident).await[0].id;
    assert!(w.llm.requests().is_empty(), "the model has not been asked");
    assert_eq!(
        w.effects(op, EffectStatus::Pending, Some(EffectKind::CallModel))
            .await
            .len(),
        1
    );

    // The caller gives up after the wait limit; the investigation is still running.
    w.clock.advance(SignedDuration::from_mins(21));
    w.settle().await;
    let waited = waiting.await.unwrap();
    assert_eq!(waited, Err(InvestigateError::TimedOut(op)));
    assert!(!w.load(op).await.operation.status.is_terminal());

    // It finishes anyway, and the report lands on the incident.
    w.gate.open();
    let snapshot = w.until_closed(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Succeeded);
    assert_eq!(w.count(&w.incident, "triage.report").await, 1);
    assert_eq!(w.llm.requests().len(), 1);
}

#[tokio::test]
async fn manual_run_replaces_a_parked_retry_instead_of_answering_already_running() {
    let w = inv(hourly(1)).await;
    take_the_slot(&w).await;
    let parked = w.start().await;
    until_skipped(&w, parked).await;
    w.until(parked, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;

    // The automatic path keeps the parked operation: its retry has a due time of its own.
    assert_eq!(
        w.ops.investigate(&w.incident, false).await.unwrap(),
        parked,
        "an event does not restart a parked retry"
    );

    // A person asking for a run does not get "already running" for up to an hour. The
    // replacement goes through the same hourly limit, so it is skipped again, not run.
    let outcome = w.spawn_wait().await.unwrap();
    assert!(
        !matches!(outcome, Err(InvestigateError::AlreadyRunning(_))),
        "{outcome:?}"
    );
    let old = w.until_closed(parked).await;
    assert_eq!(old.operation.status, OperationStatus::Cancelled);
    assert_eq!(w.llm.requests().len(), 1, "the hourly limit still holds");
}

#[tokio::test]
async fn parked_retry_shows_waiting_timer_and_states_the_cause_first() {
    let w = inv(hourly(1)).await;
    take_the_slot(&w).await;
    let op = w.start().await;
    until_skipped(&w, op).await;
    w.until(op, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;
    w.settle().await;
    let snapshot = w.load(op).await;
    assert_eq!(
        snapshot.operation.status,
        OperationStatus::WaitingTimer,
        "a parked retry is a wait, not a run in progress"
    );
    let notification = of_kind(&snapshot, StepKind::Notification)[0];
    let text = notification.progress.summary.clone().unwrap_or_default();
    assert!(
        text.starts_with("the limit of 1 triage runs per hour is reached"),
        "the cause comes first: {text}"
    );
}

#[tokio::test]
async fn failed_run_states_the_cause_then_the_retry() {
    let w = inv(hourly(10)).await;
    w.llm
        .push_err(LlmError::Unavailable("model unreachable".to_owned()));
    let op = w.start().await;
    w.until(op, |s| !of_kind(s, StepKind::Notification).is_empty())
        .await;
    w.settle().await;
    let snapshot = w.load(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::WaitingTimer);
    let text = of_kind(&snapshot, StepKind::Notification)[0]
        .progress
        .summary
        .clone()
        .unwrap_or_default();
    assert!(text.contains("model unreachable"), "{text}");
    assert!(text.contains("will retry at"), "{text}");
    assert!(text.contains("attempt 2 of 4"), "{text}");
}

#[tokio::test]
async fn manual_run_that_is_skipped_ends_failed_without_a_retry() {
    let w = inv(hourly(1)).await;
    take_the_slot(&w).await;
    let (ops, incident) = (w.ops.clone(), w.incident.clone());
    let waiting =
        tokio::spawn(async move { ops.investigate_on_request_and_wait(&incident, false).await });
    w.settle().await;
    let _ = waiting.await;
    let op = w.operations_of(&w.incident).await[0].id;
    let snapshot = w.until_closed(op).await;
    assert_eq!(snapshot.operation.status, OperationStatus::Failed);
    assert!(
        retries(&w, op).await.is_empty(),
        "a run a person asked for does not retry by itself"
    );
    assert_eq!(w.count(&w.incident, "triage.skipped").await, 1);
}

/// Three failed model calls in a row, so the breaker is open.
async fn model_down(w: &Inv) {
    for _ in 0..3 {
        w.llm
            .push_err(LlmError::Unavailable("connection refused".to_owned()));
    }
    let op = w.start().await;
    w.until_stepping(op, 300, |s| s.operation.status.is_terminal())
        .await;
    assert_eq!(w.llm.requests().len(), 3);
}

#[tokio::test]
async fn a_dead_model_is_one_fact_not_a_failure_per_queued_operation() {
    let w = inv(hourly(10)).await;
    model_down(&w).await;

    let other = w.add_incident("redis", disk(50.0)).await;
    let op = w.ops.investigate(&other, false).await.unwrap();
    let snapshot = until_skipped(&w, op).await;

    assert_eq!(
        w.llm.requests().len(),
        3,
        "the open breaker spared the model another failed call"
    );
    let text = output_text(&snapshot);
    assert!(text.contains("unreachable since"), "{text}");
    assert!(text.contains("connection refused"), "{text}");
    assert!(text.contains("tries again from"), "{text}");
    assert_eq!(
        w.advisories().await.len(),
        1,
        "one notice, not one per operation"
    );
}

#[tokio::test]
async fn a_person_asking_for_triage_probes_a_dead_model_once() {
    let w = inv(hourly(10)).await;
    model_down(&w).await;

    let other = w.add_incident("redis", disk(50.0)).await;
    w.llm.push_text(report());
    let (ops, asked) = (w.ops.clone(), other.clone());
    let waiting =
        tokio::spawn(async move { ops.investigate_on_request_and_wait(&asked, false).await });
    w.settle().await;
    let _ = waiting.await;
    assert_eq!(
        w.llm.requests().len(),
        4,
        "the manual run reached the model"
    );
}

/// The text of the first turn that was skipped.
fn output_text(snapshot: &domovoi_core::OperationSnapshot) -> String {
    turns(snapshot)
        .iter()
        .find(|t| t.progress.outcome == StepOutcome::NotRun)
        .and_then(|t| t.progress.reason.clone())
        .unwrap_or_default()
}
