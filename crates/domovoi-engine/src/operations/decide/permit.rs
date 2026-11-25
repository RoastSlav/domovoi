//! A policy permit authorized an execution: the approval step is skipped, a policy-decision step
//! records why, and the dispatch is queued.

use domovoi_core::ports::EffectKind;
use domovoi_core::{ExecutionState, StepId, StepIntent, StepOutcome, StepProgress};
use serde_json::json;

use super::DecideError;
use super::effects::{DISPATCH_ATTEMPTS, dispatch_key, step_payload};
use super::execution::awaiting;
use super::status::{Allow, gate};
use super::work::Work;
use crate::operations::audit;

/// Records the authorization. The execution row must already hold the permit.
pub(super) fn authorized_by_policy(work: &mut Work<'_>, step: StepId) -> Result<(), DecideError> {
    gate(work.op(), Allow::ACTIVE)?;
    let (execution, approval) = awaiting(work, step)?;
    let row = work
        .snap
        .execution_of(step)
        .ok_or_else(|| DecideError::Invalid("the execution has no row".into()))?
        .value
        .clone();
    let Some(stored) = row.permit() else {
        return Err(DecideError::Invalid("the execution has no permit".into()));
    };
    let permit = stored.permit();
    let summary = format!(
        "Allowed by policy version {} (rules: {})",
        permit.policy_version(),
        permit.rule_ids().join(", ")
    );
    let detail = json!({
        "plan_hash": row.plan_hash.short(),
        "decision_id": stored.decision_id(),
        "policy_version": permit.policy_version(),
        "rules": permit.rule_ids(),
    });
    // The approval step was never asked: it is skipped, not approved.
    let skipped = work.ended(
        &approval.progress,
        StepOutcome::Skipped,
        None,
        Some("authorized by policy"),
        None,
    );
    work.set_progress(approval.step.id, skipped)?;
    let now = work.now;
    work.add_step(
        StepId::new(),
        "policy",
        execution.step.parent,
        StepIntent::PolicyDecision { execution: step },
        StepProgress {
            outcome: StepOutcome::Succeeded,
            started_at: Some(now),
            ended_at: Some(now),
            summary: Some(summary),
            reason: None,
            output: None,
        },
    )?;
    let operation = work.op().id;
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
    work.audit_execution(audit::POLICY_PERMIT_ISSUED, step, detail);
    Ok(())
}
