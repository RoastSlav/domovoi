//! The result of a verification: success ends the operation, failure starts a new cycle.

use domovoi_core::ports::EffectKind;
use domovoi_core::{BudgetKind, OperationStatus, StepId, StepKind, StepOutcome, StepProgress};

use super::DecideError;
use super::effects::{BUILD_CONTEXT_ATTEMPTS, build_key, build_payload};
use super::finish::close_open_steps;
use super::status::gate_settle;
use super::work::Work;

/// A verification finished. A pass ends the operation when nothing else is open; a failure
/// starts the next cycle, or escalates when the cycle budget is spent. After the operation
/// ended, a verification that was still running is only recorded.
pub(super) fn verified(
    work: &mut Work<'_>,
    step: StepId,
    passed: bool,
    summary: &str,
) -> Result<(), DecideError> {
    let record = work.find(step)?;
    if record.step.kind != StepKind::Verification {
        return Err(DecideError::Invalid("not a verification step".into()));
    }
    let before = record.progress.outcome;
    let progress = record.progress.clone();
    if before.is_final() {
        return Err(DecideError::StepNotOpen(step));
    }
    let record_only = gate_settle(work.op(), before == StepOutcome::Running, true)?;
    if !record_only && matches!(work.op().status, OperationStatus::Paused { .. }) {
        return Err(DecideError::Paused);
    }
    let outcome = if passed {
        StepOutcome::Succeeded
    } else {
        StepOutcome::Failed
    };
    let ended = StepProgress {
        started_at: progress.started_at.or(Some(work.now)),
        ..work.ended(&progress, outcome, Some(summary), None, None)
    };
    work.set_progress(step, ended)?;
    if record_only {
        return Ok(());
    }
    if passed {
        if work.snap.open_steps().next().is_none() {
            work.op_mut().status = OperationStatus::Succeeded;
        }
        return Ok(());
    }
    let cycle = work.op().cycle;
    if work.op().budgets.is_spent(BudgetKind::Cycles, cycle) {
        close_open_steps(work, StepOutcome::Skipped, "the operation escalated", true)?;
        work.op_mut().status = OperationStatus::Escalated;
        return Ok(());
    }
    let next = cycle.saturating_add(1);
    work.op_mut().cycle = next;
    let (operation, now) = (work.op().id, work.now);
    work.effect(
        EffectKind::BuildContext,
        build_payload(operation, next),
        now,
        BUILD_CONTEXT_ATTEMPTS,
        build_key(operation, next),
    );
    Ok(())
}
