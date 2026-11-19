//! Events about executions: proposing, deciding, dispatching and settling.

use domovoi_core::ports::EffectKind;
use domovoi_core::{
    BudgetKind, EdgeKind, ExecutionIntent, ExecutionRecord, ExecutionState, JournalEpoch,
    PauseReason, StepIntent, StepKind, StepOutcome, StepProgress,
};
use domovoi_core::{StepId, StepRecord};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use super::DecideError;
use super::effects::{
    DISPATCH_ATTEMPTS, RECONCILE_ATTEMPTS, VERIFY_ATTEMPTS, WAKE_ATTEMPTS, dispatch_key,
    expiry_key, reconcile_key, step_payload, verify_key, verify_payload, wake_payload,
};
use super::events::ExecutionProposal;
use super::status::{Allow, gate, gate_settle};
use super::work::{Work, after};
use crate::operations::audit;

/// What the proposal asks to run, as an execution intent.
fn same_run(a: &ExecutionIntent, b: &ExecutionIntent) -> bool {
    a.contract == b.contract
        && a.host == b.host
        && a.check == b.check
        && a.target == b.target
        && a.args == b.args
}

/// Stores a refused Execution step: no approval step, no execution row, no budget charged.
fn refuse(
    work: &mut Work<'_>,
    proposal: ExecutionProposal,
    reason: &str,
) -> Result<(), DecideError> {
    let ExecutionProposal {
        intent,
        step,
        parent,
        ..
    } = proposal;
    let born = StepProgress {
        outcome: StepOutcome::Refused,
        started_at: None,
        ended_at: Some(work.now),
        summary: None,
        reason: Some(super::work::short_text(reason)),
        output: None,
    };
    work.add_step(
        step,
        "execution",
        parent,
        StepIntent::Execution(intent),
        born,
    )?;
    work.transition(
        step,
        ExecutionState::Proposed,
        ExecutionState::Refused,
        Some(reason),
    );
    work.audit_execution(audit::EXECUTION_REFUSED, step, json!({ "reason": reason }));
    Ok(())
}

/// Proposes a run: an Approval step, then the Execution step it gates, and the row a person
/// will approve. Anything that cannot be run is stored refused instead.
pub(super) fn propose_execution(
    work: &mut Work<'_>,
    proposal: ExecutionProposal,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE)?;
    if let Some(reason) = proposal.refused.clone() {
        return refuse(work, proposal, &reason);
    }
    let Some(plan_hash) = proposal.plan_hash else {
        return Err(DecideError::Invalid(
            "a proposal needs a plan hash unless it is refused".into(),
        ));
    };
    let intent = &proposal.intent;
    if !work
        .op()
        .is_pinned(intent.host, &intent.check, intent.target.as_ref())
    {
        return refuse(work, proposal, "target is not pinned to this operation");
    }
    let failed_before: Vec<StepId> = work
        .snap
        .steps
        .iter()
        .filter(|record| {
            matches!(
                record.progress.outcome,
                StepOutcome::Failed | StepOutcome::OutcomeUnknown
            ) && matches!(&record.step.intent, StepIntent::Execution(other) if same_run(other, intent))
        })
        .map(|record| record.step.id)
        .collect();
    let stated_change = intent.retry_of.as_ref().is_some_and(|retry| {
        !retry.changed.trim().is_empty() && failed_before.contains(&retry.step)
    });
    if !failed_before.is_empty() && !stated_change {
        return refuse(
            work,
            proposal,
            "the same run failed earlier; say what changed",
        );
    }
    let cycle = work.op().cycle;
    if work.op().budgets.is_spent(BudgetKind::Executions, cycle) {
        refuse(work, proposal, "execution budget spent")?;
        work.pause(PauseReason::Budget {
            exhausted: BudgetKind::Executions,
        });
        return Ok(());
    }
    accept(work, proposal, plan_hash)
}

/// The accepted path of a proposal.
fn accept(
    work: &mut Work<'_>,
    proposal: ExecutionProposal,
    plan_hash: domovoi_core::PlanHash,
) -> Result<(), DecideError> {
    let ExecutionProposal {
        intent,
        step,
        approval_step,
        parent,
        caused_by,
        expires_at,
        risk,
        ..
    } = proposal;
    let operation = work.op().id;
    work.charge(BudgetKind::Executions, 1);
    work.add_step(
        approval_step,
        "approval",
        parent,
        StepIntent::Approval {
            execution: step,
            plan_hash,
            expires_at,
        },
        StepProgress::pending(),
    )?;
    work.add_step(
        step,
        "execution",
        parent,
        StepIntent::Execution(intent.clone()),
        StepProgress::pending(),
    )?;
    work.add_edge(step, approval_step, EdgeKind::DependsOn)?;
    if let Some(cause) = caused_by {
        work.add_edge(step, cause, EdgeKind::CausedBy)?;
    }
    if let Some(retry) = &intent.retry_of {
        work.add_edge(step, retry.step, EdgeKind::RetryOf)?;
    }
    let row = ExecutionRecord::propose(
        &intent, step, operation, plan_hash, expires_at, risk, work.now,
    );
    work.add_execution(row);
    work.effect(
        EffectKind::Wake,
        wake_payload(operation, Some(step), "approval_expiry"),
        expires_at,
        WAKE_ATTEMPTS,
        expiry_key(step),
    );
    work.transition(
        step,
        ExecutionState::Proposed,
        ExecutionState::AwaitingApproval,
        None,
    );
    work.audit_execution(
        audit::EXECUTION_PROPOSED,
        step,
        json!({ "plan_hash": plan_hash.short(), "host": intent.host.to_string() }),
    );
    Ok(())
}

/// The Execution step and its Approval step, when both are still pending.
fn awaiting(work: &Work<'_>, step: StepId) -> Result<(StepRecord, StepRecord), DecideError> {
    let execution = work.find(step)?;
    if execution.step.kind != StepKind::Execution {
        return Err(DecideError::Invalid("not an execution step".into()));
    }
    let approval = work
        .approval_of(step)
        .ok_or(DecideError::StepNotOpen(step))?;
    if execution.progress.outcome != StepOutcome::Pending
        || approval.progress.outcome != StepOutcome::Pending
    {
        return Err(DecideError::StepNotOpen(step));
    }
    Ok((execution.clone(), approval.clone()))
}

/// A person approved: the approval step succeeds and the dispatch is queued. The execution
/// row must already hold the approval.
pub(super) fn authorized(work: &mut Work<'_>, step: StepId) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE)?;
    let (_, approval) = awaiting(work, step)?;
    let row = work
        .snap
        .execution_of(step)
        .ok_or_else(|| DecideError::Invalid("the execution has no row".into()))?
        .value
        .clone();
    let Some(stored) = row.approval() else {
        return Err(DecideError::Invalid("the execution has no approval".into()));
    };
    let human = stored.approval();
    let detail = json!({
        "plan_hash": row.plan_hash.short(),
        "user": human.user().to_string(),
        "channel": serde_json::to_value(human.channel()).unwrap_or_default(),
        "session_ref": stored.provenance().session_ref(),
        "client": stored.provenance().client(),
    });
    let done = StepProgress {
        outcome: StepOutcome::Succeeded,
        started_at: Some(work.now),
        ended_at: Some(work.now),
        summary: None,
        reason: None,
        output: None,
    };
    work.set_progress(approval.step.id, done)?;
    let (operation, now) = (work.op().id, work.now);
    work.effect(
        EffectKind::Dispatch,
        step_payload(operation, step),
        now,
        DISPATCH_ATTEMPTS,
        dispatch_key(step),
    );
    work.transition(
        step,
        ExecutionState::AwaitingApproval,
        ExecutionState::Authorized,
        None,
    );
    work.audit_execution(audit::EXECUTION_APPROVED, step, detail);
    Ok(())
}

/// Closes an unanswered approval and its execution with the same words.
fn close_approval(
    work: &mut Work<'_>,
    step: StepId,
    outcome: StepOutcome,
    reason: &str,
    kind: &'static str,
    to: ExecutionState,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE_PAUSED)?;
    let (execution, approval) = awaiting(work, step)?;
    let approval_done = work.ended(&approval.progress, outcome, None, Some(reason), None);
    let execution_done = work.ended(&execution.progress, outcome, None, Some(reason), None);
    work.set_progress(approval.step.id, approval_done)?;
    work.set_progress(step, execution_done)?;
    work.transition(step, ExecutionState::AwaitingApproval, to, Some(reason));
    work.audit_execution(kind, step, json!({}));
    Ok(())
}

/// A person denied: the approval and the execution are refused.
pub(super) fn denied(work: &mut Work<'_>, step: StepId) -> Result<(), DecideError> {
    close_approval(
        work,
        step,
        StepOutcome::Refused,
        "denied by a person",
        audit::EXECUTION_DENIED,
        ExecutionState::Refused,
    )
}

/// The approval window closed: the approval and the execution did not run.
pub(super) fn expired(work: &mut Work<'_>, step: StepId) -> Result<(), DecideError> {
    close_approval(
        work,
        step,
        StepOutcome::NotRun,
        "the approval expired before the action was dispatched",
        audit::EXECUTION_EXPIRED,
        ExecutionState::NotRun,
    )
}

/// The agent is not there: no step changes, but the operation now waits for it.
pub(super) fn waiting_for_agent(work: &mut Work<'_>, step: StepId) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE_PAUSED)?;
    let record = work.find(step)?;
    if record.step.kind != StepKind::Execution || record.progress.outcome != StepOutcome::Pending {
        return Err(DecideError::StepNotOpen(step));
    }
    let now = work.now;
    let row = work
        .snap
        .executions
        .iter_mut()
        .find(|row| row.value.step == step)
        .ok_or_else(|| DecideError::Invalid("the execution has no row".into()))?;
    if row.value.approval().is_none() {
        return Err(DecideError::Invalid("the execution has no approval".into()));
    }
    row.value.waiting_since.get_or_insert(now);
    Ok(())
}

/// The execution was handed to its agent: the step runs and a reconcile is queued.
pub(super) fn dispatched(
    work: &mut Work<'_>,
    step: StepId,
    epoch: JournalEpoch,
    reconcile_after: Timestamp,
) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE)?;
    let record = work.find(step)?;
    if record.step.kind != StepKind::Execution || record.progress.outcome != StepOutcome::Pending {
        return Err(DecideError::StepNotOpen(step));
    }
    let progress = record.progress.clone();
    match work.snap.execution_of(step) {
        Some(row) if row.value.approval().is_some() => {}
        _ => {
            return Err(DecideError::Invalid("the execution has no approval".into()));
        }
    }
    let running = StepProgress {
        outcome: StepOutcome::Running,
        started_at: Some(work.now),
        ..progress
    };
    work.set_progress(step, running)?;
    let operation = work.op().id;
    work.effect(
        EffectKind::Reconcile,
        step_payload(operation, step),
        reconcile_after,
        RECONCILE_ATTEMPTS,
        reconcile_key(step),
    );
    work.transition(
        step,
        ExecutionState::Authorized,
        ExecutionState::Dispatched,
        None,
    );
    work.audit_execution(
        audit::EXECUTION_DISPATCHED,
        step,
        json!({ "epoch": epoch.as_uuid().to_string() }),
    );
    Ok(())
}

/// The audit kind that records an execution ending as `outcome`.
fn settle_kind(outcome: StepOutcome) -> Option<&'static str> {
    Some(match outcome {
        StepOutcome::Refused => audit::EXECUTION_REFUSED,
        StepOutcome::NotRun => audit::EXECUTION_NOT_RUN,
        StepOutcome::Unavailable => audit::EXECUTION_UNAVAILABLE,
        StepOutcome::Succeeded => audit::EXECUTION_SUCCEEDED,
        StepOutcome::Failed => audit::EXECUTION_FAILED,
        StepOutcome::OutcomeUnknown => audit::EXECUTION_OUTCOME_UNKNOWN,
        _ => return None,
    })
}

/// An execution ended. A success may queue its verification; an unknown outcome pauses the
/// operation. After the operation ended, a run that was still going is only recorded.
pub(super) fn settled(
    work: &mut Work<'_>,
    step: StepId,
    outcome: StepOutcome,
    reason: Option<String>,
    output: Option<String>,
    verify_after: Option<SignedDuration>,
) -> Result<(), DecideError> {
    let record = work.find(step)?;
    if record.step.kind != StepKind::Execution {
        return Err(DecideError::Invalid("not an execution step".into()));
    }
    let from = record.progress.outcome;
    let progress = record.progress.clone();
    if from.is_final() {
        return Err(DecideError::StepNotOpen(step));
    }
    let record_only = gate_settle(work.op(), from == StepOutcome::Running, false)?;
    let allowed = match from {
        StepOutcome::Pending => matches!(
            outcome,
            StepOutcome::Refused | StepOutcome::NotRun | StepOutcome::Unavailable
        ),
        _ => matches!(
            outcome,
            StepOutcome::Succeeded
                | StepOutcome::Failed
                | StepOutcome::OutcomeUnknown
                | StepOutcome::NotRun
        ),
    };
    let Some(kind) = settle_kind(outcome).filter(|_| allowed) else {
        return Err(DecideError::Invalid(format!(
            "an execution cannot go from {} to {}",
            from.as_str(),
            outcome.as_str()
        )));
    };
    let before = work.exec_state(step).unwrap_or(ExecutionState::Authorized);
    let ended = work.ended(
        &progress,
        outcome,
        None,
        reason.as_deref(),
        output.as_deref(),
    );
    work.set_progress(step, ended)?;
    let after_state = work.exec_state(step).unwrap_or(before);
    work.transition(step, before, after_state, reason.as_deref());
    let kind = if outcome == StepOutcome::Refused
        && reason
            .as_deref()
            .is_some_and(|text| text.starts_with(audit::LOOP_GUARD_REASON))
    {
        audit::EXECUTION_BLOCKED
    } else {
        kind
    };
    work.audit_execution(kind, step, json!({ "reason": reason }));
    if record_only {
        return Ok(());
    }
    if outcome == StepOutcome::Succeeded
        && let Some(delay) = verify_after
    {
        queue_verification(work, step, delay)?;
    }
    if outcome == StepOutcome::OutcomeUnknown {
        work.pause(PauseReason::OutcomeUnknown { step });
    }
    Ok(())
}

/// Adds the Verification step that checks Execution step `step` after `delay`.
fn queue_verification(
    work: &mut Work<'_>,
    step: StepId,
    delay: SignedDuration,
) -> Result<(), DecideError> {
    let verification = work.new_step_id();
    let delay = delay.max(SignedDuration::ZERO);
    work.add_step(
        verification,
        "verify",
        Some(step),
        StepIntent::Verification {
            execution: step,
            delay_secs: u64::try_from(delay.as_secs()).unwrap_or(0),
        },
        StepProgress::pending(),
    )?;
    work.add_edge(verification, step, EdgeKind::Verifies)?;
    let (operation, due) = (work.op().id, after(work.now, delay));
    work.effect(
        EffectKind::Verify,
        verify_payload(operation, verification, step),
        due,
        VERIFY_ATTEMPTS,
        verify_key(step),
    );
    Ok(())
}
