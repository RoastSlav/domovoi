//! Events about the operation as a whole: admit, cancel, pause, resume, finish and tick.

use domovoi_core::{
    BudgetKind, FinishOutcome, OperationStatus, PauseReason, StepId, StepKind, StepOutcome,
};
use serde_json::json;

use super::DecideError;
use super::status::{Allow, gate};
use super::work::{Work, short_text};
use crate::operations::audit;

/// Leaves the queue. A wall-time limit starts a wake that measures it.
pub(super) fn admit(work: &mut Work<'_>) -> Result<(), DecideError> {
    gate(work.op(), Allow::QUEUED_ONLY)?;
    let now = work.now;
    let op = work.op_mut();
    op.status = OperationStatus::Running;
    op.started_at = Some(now);
    if let Some(limit) = op.budgets.limits.wall_seconds {
        work.wall_wake(now, limit);
    }
    Ok(())
}

/// Moves every open step to `outcome` with `reason`, keeping a dispatched execution (a
/// running Execution step) as it is when `keep_running_executions` is set.
pub(super) fn close_open_steps(
    work: &mut Work<'_>,
    outcome: StepOutcome,
    reason: &str,
    keep_running_executions: bool,
) -> Result<(), DecideError> {
    let open: Vec<(StepId, StepKind, StepOutcome)> = work
        .snap
        .open_steps()
        .map(|r| (r.step.id, r.step.kind, r.progress.outcome))
        .collect();
    for (id, kind, was) in open {
        if keep_running_executions && kind == StepKind::Execution && was == StepOutcome::Running {
            continue;
        }
        let previous = work.find(id)?.progress.clone();
        let from = (kind == StepKind::Execution)
            .then(|| work.exec_state(id))
            .flatten();
        let ended = work.ended(&previous, outcome, None, Some(reason), None);
        work.set_progress(id, ended)?;
        if let (Some(from), Some(to)) = (from, work.exec_state(id)) {
            work.transition(id, from, to, Some(reason));
        }
    }
    Ok(())
}

/// Stops the operation. A dispatched execution is not recalled: it stays running until its
/// result is recorded.
pub(super) fn cancel(work: &mut Work<'_>, reason: &str) -> Result<(), DecideError> {
    gate(work.op(), Allow::OPEN)?;
    let text = short_text(reason);
    close_open_steps(work, StepOutcome::Cancelled, &text, true)?;
    work.op_mut().status = OperationStatus::Cancelled;
    work.audit_operation(audit::OPERATION_CANCELLED, None, json!({ "reason": text }));
    Ok(())
}

/// Ends the operation. Refused while an execution runs, because its result would be lost.
pub(super) fn finish(
    work: &mut Work<'_>,
    outcome: FinishOutcome,
    summary: &str,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE_PAUSED)?;
    let running =
        work.snap.steps.iter().find(|r| {
            r.step.kind == StepKind::Execution && r.progress.outcome == StepOutcome::Running
        });
    if let Some(record) = running {
        return Err(DecideError::ExecutionInFlight(record.step.id));
    }
    close_open_steps(work, StepOutcome::Skipped, "the operation ended", false)?;
    work.summary = Some(short_text(summary));
    work.op_mut().status = outcome.status();
    Ok(())
}

/// Stops the operation until a person resumes it.
pub(super) fn pause(work: &mut Work<'_>, reason: PauseReason) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE)?;
    work.pause(reason);
    Ok(())
}

/// Resumes a paused operation after raising the named limits. A budget pause needs the
/// budget that stopped it to have room again.
pub(super) fn resume(
    work: &mut Work<'_>,
    extend: &[(BudgetKind, Option<u64>)],
) -> Result<(), DecideError> {
    gate(work.op(), Allow::PAUSED_ONLY)?;
    for (kind, limit) in extend {
        work.op_mut().budgets.extend(*kind, *limit);
    }
    let cycle = work.op().cycle;
    if let Some(PauseReason::Budget { exhausted }) = work.op().status.pause_reason().cloned()
        && work.op().budgets.is_spent(exhausted, cycle)
    {
        return Err(DecideError::BudgetStillSpent(exhausted));
    }
    if let Some((_, Some(limit))) = extend
        .iter()
        .find(|(kind, _)| *kind == BudgetKind::WallTime)
        && let Some(started) = work.op().started_at
    {
        work.wall_wake(started, *limit);
    }
    work.op_mut().status = OperationStatus::Running;
    work.audit_operation(audit::OPERATION_RESUMED, None, json!({}));
    Ok(())
}

/// Measures wall-clock time against the budget and pauses when it is past.
pub(super) fn tick(work: &mut Work<'_>) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE_PAUSED)?;
    let Some(started) = work.op().started_at else {
        return Err(DecideError::Invalid("the operation has not started".into()));
    };
    let elapsed = u64::try_from(work.now.duration_since(started).as_secs()).unwrap_or(0);
    let used = &mut work.op_mut().budgets.used;
    used.wall_seconds = used.wall_seconds.max(elapsed);
    if let Some(exhausted) = work.op().budgets.overrun() {
        work.pause(PauseReason::Budget { exhausted });
    }
    Ok(())
}
