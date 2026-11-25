//! Timed wakes: the retry of a dispatch that waits for its agent, the end of an approval
//! window, the end of a question's wait and the wall-time measurement.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{OperationSnapshot, OperationStatus, StepId, StepKind, StepOutcome};
use jiff::SignedDuration;

use super::dispatch::pending_execution;
use super::{Address, Ending, address, after, ask, defer, finish, held, load, outcome, settle};
use super::{spec, unreadable};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::decide::{
    DISPATCH_ATTEMPTS, WAKE_ATTEMPTS, dispatch_key, step_payload, wake_payload,
};
use crate::operations::writer::{Held, OpShared, OperationCommand};

/// Does what a wake is for, by the purpose its payload names.
pub(super) struct WakeHandler {
    shared: Arc<OpShared>,
}

impl WakeHandler {
    /// A handler over the operation service's writer and ports.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for WakeHandler {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let (Some(Address { operation, step }), Some(purpose)) = (
            address(effect),
            effect.payload.get("purpose").and_then(|p| p.as_str()),
        ) else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        if snapshot.operation.status.is_terminal() {
            return finish(shared, held).await;
        }
        match (purpose, step) {
            ("retry_dispatch", Some(step)) => retry_dispatch(shared, held, &snapshot, step).await,
            ("approval_expiry", Some(step)) => approval_expiry(shared, held, &snapshot, step).await,
            ("question_timeout", Some(step)) => {
                question_timeout(shared, held, &snapshot, step).await
            }
            ("wall_time", None) => wall_time(shared, held, &snapshot).await,
            _ => EffectOutcome::Dead {
                error: format!("unknown wake purpose {purpose}"),
            },
        }
    }
}

/// An approved execution waited for its agent: send it when the agent is back, give up when
/// the wait limit passed.
async fn retry_dispatch(
    shared: &OpShared,
    held: Held,
    snapshot: &OperationSnapshot,
    step: StepId,
) -> EffectOutcome {
    let operation = snapshot.operation.id;
    let (true, Some(row)) = (
        pending_execution(snapshot, step),
        snapshot.execution_of(step).map(|stored| &stored.value),
    ) else {
        return finish(shared, held).await;
    };
    let now = shared.deps.clock.now();
    let since = row.waiting_since.unwrap_or(now);
    let limit = since
        .checked_add(shared.settings.agent_wait_limit)
        .unwrap_or(now);
    if now >= limit {
        let ending = Ending::new(operation, step, StepOutcome::Unavailable)
            .because("the agent did not come back in time")
            .holding(held);
        return outcome(settle(shared, ending).await);
    }
    let next = if shared.deps.observer.observe_snapshot(row.host).is_some() {
        spec(
            EffectKind::Dispatch,
            step_payload(operation, step),
            now,
            DISPATCH_ATTEMPTS,
            Some(dispatch_key(step)),
        )
    } else {
        spec(
            EffectKind::Wake,
            wake_payload(operation, Some(step), "retry_dispatch"),
            after(shared, shared.settings.retry_interval),
            WAKE_ATTEMPTS,
            Some(format!("wake:retry:{step}")),
        )
    };
    defer(shared, held, operation, Some(step), next, false).await
}

/// The approval window of an execution ended: if nobody decided, it expires.
async fn approval_expiry(
    shared: &OpShared,
    held: Held,
    snapshot: &OperationSnapshot,
    step: StepId,
) -> EffectOutcome {
    let operation = snapshot.operation.id;
    let awaiting = pending_execution(snapshot, step)
        && snapshot
            .execution_of(step)
            .is_some_and(|stored| !stored.value.is_authorized());
    let Some(row) = snapshot.execution_of(step).filter(|_| awaiting) else {
        return finish(shared, held).await;
    };
    if shared.deps.clock.now() < row.value.expires_at {
        let again = spec(
            EffectKind::Wake,
            wake_payload(operation, Some(step), "approval_expiry"),
            row.value.expires_at,
            WAKE_ATTEMPTS,
            None,
        );
        return defer(shared, held, operation, None, again, false).await;
    }
    let answered = ask(shared, |reply| OperationCommand::Expire {
        held,
        operation,
        step,
        reply,
    })
    .await;
    outcome(answered)
}

/// A question's wait ended: a question nobody answered is not run.
async fn question_timeout(
    shared: &OpShared,
    held: Held,
    snapshot: &OperationSnapshot,
    step: StepId,
) -> EffectOutcome {
    let operation = snapshot.operation.id;
    let open = |record: &domovoi_core::StepRecord| {
        record.step.kind == StepKind::Question && !record.progress.outcome.is_final()
    };
    if snapshot.operation.status == OperationStatus::Queued && snapshot.step(step).is_some_and(open)
    {
        // The question cannot time out before its operation has a slot: look again later.
        let again = spec(
            EffectKind::Wake,
            wake_payload(operation, Some(step), "question_timeout"),
            after(shared, shared.settings.retry_interval),
            WAKE_ATTEMPTS,
            None,
        );
        return defer(shared, held, operation, None, again, false).await;
    }
    if !snapshot.step(step).is_some_and(open) {
        return finish(shared, held).await;
    }
    let answered = ask(shared, |reply| OperationCommand::FinishStep {
        held,
        operation,
        step,
        outcome: StepOutcome::NotRun,
        summary: None,
        reason: Some("no answer before the timeout".to_owned()),
        output: None,
        reply,
    })
    .await;
    outcome(answered)
}

/// The wall-time limit may have passed: measure it. A wake that fires exactly at the limit
/// measures nothing over it, so it looks again a second later.
async fn wall_time(shared: &OpShared, held: Held, snapshot: &OperationSnapshot) -> EffectOutcome {
    let operation = snapshot.operation.id;
    let now = shared.deps.clock.now();
    let limit = snapshot.operation.budgets.limits.wall_seconds;
    if let (Some(limit), Some(started)) = (limit, snapshot.operation.started_at) {
        let limit = SignedDuration::from_secs(i64::try_from(limit).unwrap_or(i64::MAX));
        let over_at = started
            .checked_add(limit)
            .and_then(|at| at.checked_add(SignedDuration::from_secs(1)))
            .unwrap_or(now);
        if now < over_at {
            let again = spec(
                EffectKind::Wake,
                wake_payload(operation, None, "wall_time"),
                over_at,
                WAKE_ATTEMPTS,
                None,
            );
            return defer(shared, held, operation, None, again, false).await;
        }
    }
    let answered = ask(shared, |reply| OperationCommand::Tick {
        held,
        operation,
        reply,
    })
    .await;
    outcome(answered)
}
