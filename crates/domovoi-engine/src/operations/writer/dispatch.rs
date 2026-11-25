//! The writer's last word before an execution is handed to its agent: re-prove the
//! authorization, the freshness of the plan, the capability, the session and the plan hash,
//! then record the hand-off or settle the execution without one.

use domovoi_core::capability::Availability;
use domovoi_core::ports::DISABLED_REASON;
use domovoi_core::{
    AgentSessionId, ExecutionRecord, IncidentStatus, JournalEpoch, OperationId, OperationSnapshot,
    OperationStatus, PlanHash, StepId, StepKind, StepOutcome,
};
use jiff::Timestamp;

use super::OpShared;
use super::commands::{DispatchAnswer, Held};
use super::commit::{apply, queue_reconcile, release};
use super::execution::{Settled, dispatched, settle};
use crate::operations::decide::DecideError;
use crate::operations::error::OperationError;

/// What the dispatch command asked, as the checks read it.
pub(super) struct Request {
    pub(super) held: Held,
    pub(super) operation: OperationId,
    pub(super) step: StepId,
    pub(super) plan: PlanHash,
    pub(super) session: AgentSessionId,
    pub(super) epoch: JournalEpoch,
}

/// What the checks decided.
enum Verdict {
    /// Everything holds: hand it over.
    Go,
    /// Do not hand it over; the execution ends this way, for this reason.
    Stop(StepOutcome, String),
}

/// A verdict that stops the dispatch.
fn stop(outcome: StepOutcome, reason: &str) -> Option<Verdict> {
    Some(Verdict::Stop(outcome, reason.to_owned()))
}

/// Checks the execution and commits `ExecutionDispatched` or a settlement together with the
/// completion of the dispatch effect.
pub(super) async fn dispatch(
    shared: &OpShared,
    request: Request,
) -> Result<DispatchAnswer, OperationError> {
    let Request {
        held,
        operation,
        step,
        plan,
        session,
        epoch,
    } = request;
    let verdict = match judge(shared, operation, step, plan, session, epoch).await {
        Ok(verdict) => verdict,
        Err(err) => {
            release(shared, held, &err).await;
            return Err(err);
        }
    };
    match verdict {
        Verdict::Go => {
            apply(shared, dispatched(shared, held, operation, step, epoch)).await?;
            Ok(DispatchAnswer::Dispatched(epoch))
        }
        Verdict::Stop(outcome, reason) => {
            let settled = Settled {
                held: Some(held),
                operation,
                step,
                outcome,
                reason: Some(reason),
                output: None,
                verify_after: None,
            };
            apply(shared, settle(shared, settled)).await?;
            Ok(DispatchAnswer::Settled(outcome))
        }
    }
}

/// Reads what the checks need and runs them in order.
async fn judge(
    shared: &OpShared,
    operation: OperationId,
    step: StepId,
    plan: PlanHash,
    session: AgentSessionId,
    epoch: JournalEpoch,
) -> Result<Verdict, OperationError> {
    let now = shared.deps.clock.now();
    let snapshot = shared
        .deps
        .operations
        .load(operation)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?
        .ok_or(OperationError::NotFound(operation))?;
    let record = snapshot.step(step).ok_or(DecideError::UnknownStep(step))?;
    if record.step.kind != StepKind::Execution || record.progress.outcome != StepOutcome::Pending {
        return Err(DecideError::StepNotOpen(step).into());
    }
    let row = snapshot
        .execution_of(step)
        .ok_or_else(|| DecideError::Invalid("the execution has no row".into()))?
        .value
        .clone();
    if let Some(verdict) = authorization(&row) {
        return Ok(verdict);
    }
    if let Some(verdict) = freshness(shared, &snapshot, &row, now).await? {
        return Ok(verdict);
    }
    if let Some(verdict) = capability(shared, &row).await {
        return Ok(verdict);
    }
    if let Some(verdict) = agent(shared, &row, session, epoch).await? {
        return Ok(verdict);
    }
    if plan != row.plan_hash {
        return Ok(Verdict::Stop(
            StepOutcome::Refused,
            "the plan changed after approval".to_owned(),
        ));
    }
    Ok(Verdict::Go)
}

/// A stored human approval or permit that names this plan, or the execution is refused.
fn authorization(row: &ExecutionRecord) -> Option<Verdict> {
    // A stored permit is checked against the plan it names; its expiry and the pause switch are
    // checked by the dispatch itself, which reads them fresh.
    if let Some(stored) = row.permit() {
        if stored.permit().plan_hash() != row.plan_hash {
            return stop(
                StepOutcome::Refused,
                "the stored permit contradicts the plan",
            );
        }
        return None;
    }
    let Some(approval) = row.approval() else {
        return stop(
            StepOutcome::Refused,
            "the execution has no valid human authorization",
        );
    };
    if approval.displayed() != row.plan_hash {
        return stop(
            StepOutcome::Refused,
            "the stored approval contradicts the plan",
        );
    }
    None
}

/// The operation is running, the approval window is open and no linked incident is gone or
/// resolved. A paused operation is not a verdict: the dispatch waits for the resume.
async fn freshness(
    shared: &OpShared,
    snapshot: &OperationSnapshot,
    row: &ExecutionRecord,
    now: Timestamp,
) -> Result<Option<Verdict>, OperationError> {
    match &snapshot.operation.status {
        OperationStatus::Paused { .. } => return Err(DecideError::Paused.into()),
        OperationStatus::Queued => {
            return Ok(stop(StepOutcome::NotRun, "the operation is not running"));
        }
        status if status.is_terminal() => return Err(DecideError::OperationClosed.into()),
        _ => {}
    }
    if now >= row.expires_at {
        return Ok(stop(
            StepOutcome::NotRun,
            "the approval expired before the action was dispatched",
        ));
    }
    for incident in &snapshot.incidents {
        let found = shared
            .deps
            .incidents
            .get(*incident)
            .await
            .map_err(|err| OperationError::Store(err.to_string()))?;
        match found {
            None => return Ok(stop(StepOutcome::Refused, "the incident no longer exists")),
            Some(found) if found.status() == IncidentStatus::Resolved => {
                return Ok(stop(
                    StepOutcome::Refused,
                    "the incident was resolved before the action was dispatched",
                ));
            }
            Some(_) => {}
        }
    }
    Ok(None)
}

/// Only a `skill.<name>` contract runs, and only while the owner enables it and the hub
/// says it is available on the host.
async fn capability(shared: &OpShared, row: &ExecutionRecord) -> Option<Verdict> {
    if !row.contract.is_skill() {
        return stop(StepOutcome::Unavailable, "contract_not_executable");
    }
    let registry = &shared.deps.registry;
    if !registry.enabled(row.host, &row.contract) {
        return stop(StepOutcome::Refused, DISABLED_REASON);
    }
    match registry.availability(row.host, &row.contract).await {
        Availability::Available => None,
        other => Some(Verdict::Stop(StepOutcome::Unavailable, other.describe())),
    }
}

/// The session the plan was made for is still the live one, under the same journal epoch,
/// and the enrolled agent behind it is still the host's current one.
async fn agent(
    shared: &OpShared,
    row: &ExecutionRecord,
    session: AgentSessionId,
    epoch: JournalEpoch,
) -> Result<Option<Verdict>, OperationError> {
    let changed = "the agent session changed before dispatch";
    let Some(live) = shared.deps.observer.observe_snapshot(row.host) else {
        return Ok(stop(StepOutcome::Refused, changed));
    };
    if live.session != session || live.journal_epoch != epoch {
        return Ok(stop(StepOutcome::Refused, changed));
    }
    let enrolled = shared
        .deps
        .hosts
        .live_agent(row.host)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?;
    let Some(enrolled) = enrolled else {
        return Ok(stop(
            StepOutcome::Refused,
            "the host has no enrolled agent; the plan can no longer be verified",
        ));
    };
    if enrolled.agent != live.agent {
        return Ok(stop(
            StepOutcome::Refused,
            "the host's enrolled agent changed before dispatch",
        ));
    }
    Ok(None)
}

/// Queues a reconcile for each execution dispatched to `host`.
pub(super) async fn reconcile_host(shared: &OpShared, host: domovoi_core::HostId) {
    let rows = match shared.deps.operations.dispatched_on(host).await {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(%err, %host, "could not list the executions dispatched to a host");
            return;
        }
    };
    for row in rows {
        if let Err(err) = queue_reconcile(shared, row.operation, row.step).await {
            tracing::warn!(%err, step = %row.step, "could not queue a reconcile");
        }
    }
}
