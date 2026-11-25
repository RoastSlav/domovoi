//! Dispatching an approved execution: the checks that decide whether to wait, settle or send,
//! then the one dispatch boundary.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::capability::Availability;
use domovoi_core::ports::{ClaimToken, DISABLED_REASON, EffectKind, EffectRecord};
use domovoi_core::{OperationSnapshot, StepKind, StepOutcome};

use super::{Address, Ending, address, after, defer, finish, held, load, outcome, settle, spec};
use super::{still_held, unreadable};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::decide::{DISPATCH_ATTEMPTS, WAKE_ATTEMPTS, dispatch_key, wake_payload};
use crate::operations::execute::ExecuteError;
use crate::operations::writer::{Held, OpShared};

/// The reason an approval gives up when the window closed before the run could be sent.
const EXPIRED: &str = "the approval expired before the action was dispatched";

/// Sends an approved Execution step to its agent, or waits, or settles it.
pub(super) struct DispatchHandler {
    shared: Arc<OpShared>,
}

impl DispatchHandler {
    /// A handler over the operation service's writer and ports.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for DispatchHandler {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let Some(Address {
            operation,
            step: Some(step),
        }) = address(effect)
        else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        if !pending_execution(&snapshot, step) {
            return finish(shared, held).await;
        }
        let Some(row) = snapshot
            .execution_of(step)
            .map(|stored| stored.value.clone())
        else {
            return EffectOutcome::Dead {
                error: "the execution has no row".to_owned(),
            };
        };
        let ending = |result| Ending::new(operation, step, result).holding(held);
        if snapshot.operation.status.pause_reason().is_some() {
            let again = spec(
                EffectKind::Dispatch,
                effect.payload.clone(),
                after(shared, shared.settings.retry_interval),
                DISPATCH_ATTEMPTS,
                Some(dispatch_key(step)),
            );
            return defer(shared, held, operation, Some(step), again, false).await;
        }
        let registry = &shared.deps.registry;
        let is_unit_restart = row.contract.as_str() == "unit.restart";
        if !row.contract.is_skill() && !is_unit_restart {
            let ending = ending(StepOutcome::Unavailable).because("contract_not_executable");
            return outcome(settle(shared, ending).await);
        }
        if !registry.enabled(row.host, &row.contract) {
            let ending = ending(StepOutcome::Refused).because(DISABLED_REASON);
            return outcome(settle(shared, ending).await);
        }
        match registry.availability(row.host, &row.contract).await {
            Availability::Available => {}
            Availability::HostUnknown | Availability::HostAbsent { .. } => {
                return wait_for_agent(shared, held, &snapshot, step).await;
            }
            Availability::HostIncompatible { .. } => {
                let ending = ending(StepOutcome::Unavailable).because("agent upgrade required");
                return outcome(settle(shared, ending).await);
            }
            other => {
                let ending = ending(StepOutcome::Unavailable).because(other.describe());
                return outcome(settle(shared, ending).await);
            }
        }
        let Some(session) = shared.deps.observer.observe_snapshot(row.host) else {
            return wait_for_agent(shared, held, &snapshot, step).await;
        };
        // The one place the boundary type is built: a row with neither a person's approval nor a
        // stored permit is refused here, whichever contract it names.
        let authorized = match crate::operations::execute::AuthorizedStep::of(row.clone()) {
            Ok(authorized) => authorized,
            Err(err) => {
                let ending = ending(StepOutcome::Refused).because(err.to_string());
                return outcome(settle(shared, ending).await);
            }
        };
        if is_unit_restart {
            let sent = crate::operations::execute_unit::execute_unit_restart(
                shared, held, authorized, session,
            )
            .await;
            return concluded(shared, held, operation, step, sent).await;
        }
        let Some(skill) = row
            .contract
            .skill_name()
            .and_then(|name| session.skills.iter().find(|skill| skill.name == name))
            .cloned()
        else {
            let ending =
                ending(StepOutcome::Unavailable).because("the agent does not offer this skill");
            return outcome(settle(shared, ending).await);
        };
        let args = match skill.validate_args(&row.args) {
            Ok(args) => args,
            Err(err) => {
                let reason = format!("the arguments are no longer valid: {err}");
                return outcome(settle(shared, ending(StepOutcome::Refused).because(reason)).await);
            }
        };
        // Only the unit restart may be authorized by a permit; a skill never is.
        if row.permit().is_some() {
            let reason = "a policy permit cannot authorize a skill";
            return outcome(settle(shared, ending(StepOutcome::Refused).because(reason)).await);
        }
        let sent = crate::operations::execute::execute_step(
            shared, held, authorized, skill, args, session,
        )
        .await;
        concluded(shared, held, operation, step, sent).await
    }
}

/// What the effect reports once a dispatch returned: handled, settled as expired, or retried.
async fn concluded(
    shared: &OpShared,
    held: Held,
    operation: domovoi_core::OperationId,
    step: domovoi_core::StepId,
    sent: Result<(), ExecuteError>,
) -> EffectOutcome {
    match sent {
        Ok(()) => EffectOutcome::Handled,
        Err(ExecuteError::Expired) => {
            let ending = Ending::new(operation, step, StepOutcome::NotRun)
                .because(EXPIRED)
                .holding(held);
            outcome(settle(shared, ending).await)
        }
        Err(err) => {
            tracing::debug!(%err, %step, "an execution was not dispatched");
            if still_held(shared, held).await {
                EffectOutcome::Retry {
                    error: err.to_string(),
                }
            } else {
                EffectOutcome::Handled
            }
        }
    }
}

/// Whether `step` is an Execution step that has not been sent yet.
pub(super) fn pending_execution(snapshot: &OperationSnapshot, step: domovoi_core::StepId) -> bool {
    !snapshot.operation.status.is_terminal()
        && snapshot.step(step).is_some_and(|record| {
            record.step.kind == StepKind::Execution
                && record.progress.outcome == StepOutcome::Pending
        })
}

/// The agent is not there: the operation waits for it, and a wake looks again soon.
async fn wait_for_agent(
    shared: &OpShared,
    held: Held,
    snapshot: &OperationSnapshot,
    step: domovoi_core::StepId,
) -> EffectOutcome {
    let operation = snapshot.operation.id;
    let wake = spec(
        EffectKind::Wake,
        wake_payload(operation, Some(step), "retry_dispatch"),
        after(shared, shared.settings.retry_interval),
        WAKE_ATTEMPTS,
        Some(format!("wake:retry:{step}")),
    );
    defer(shared, held, operation, Some(step), wake, true).await
}
