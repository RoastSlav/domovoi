//! Reconciling a dispatched execution with the agent's journal.
//!
//! A run whose result never came back stays dispatched. This handler asks the agent's journal
//! what became of it and settles the step, unless the journal was reset since: then nobody can
//! know, and the operation pauses for a person.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord, UnknownReason};
use domovoi_core::{ExecutionId, ExecutionRecord, OperationId, StepId, StepKind, StepOutcome};

use super::unreadable;
use super::{Address, Ending, address, after, defer, finish, held, load, outcome, settle, spec};
use crate::actions::reconcile::{NOT_RUN_REASON, ReconcileOutcome, from_answer, reason_text};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::decide::{RECONCILE_ATTEMPTS, reconcile_key, step_payload};
use crate::operations::execute::verify_delay;
use crate::operations::writer::{Held, OpShared};

/// Settles a dispatched Execution step from the agent's journal.
pub(super) struct ReconcileHandler {
    shared: Arc<OpShared>,
}

impl ReconcileHandler {
    /// A handler over the operation service's writer and ports.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for ReconcileHandler {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let Some(Address {
            operation,
            step: Some(step),
        }) = address(&effect.payload)
        else {
            return unreadable();
        };
        let Some(row) = running_row(shared, operation, step).await else {
            return finish(shared, held).await;
        };
        let Some((recorded, _)) = row.dispatched else {
            let ending = unknown(operation, step, UnknownReason::EpochUnknown, held);
            return outcome(settle(shared, ending).await);
        };
        let host = row.host;
        let Some(live) = shared.deps.journal.epoch_of(host) else {
            return again(shared, held, operation, step).await;
        };
        if live != recorded {
            let ending = unknown(operation, step, UnknownReason::EpochChanged, held);
            return outcome(settle(shared, ending).await);
        }
        let wanted = ExecutionId::from_step(step).as_action();
        let deadline = after(shared, shared.settings.recheck_deadline);
        let asked = shared
            .deps
            .journal
            .status_batch(host, vec![wanted], recorded, deadline)
            .await;
        let answers = match asked {
            Ok(answers) => answers,
            Err(err) => {
                tracing::warn!(%err, %host, %step, "could not ask the agent about a dispatched run");
                if effect.attempt < effect.max_attempts {
                    return EffectOutcome::Retry {
                        error: err.to_string(),
                    };
                }
                return again(shared, held, operation, step).await;
            }
        };
        if answers.epoch != recorded {
            let ending = unknown(operation, step, UnknownReason::EpochChanged, held);
            return outcome(settle(shared, ending).await);
        }
        let answer = answers
            .answers
            .into_iter()
            .find(|entry| entry.execution == wanted)
            .and_then(|entry| from_answer(entry.answer));
        let Some(settled) = answer else {
            return again(shared, held, operation, step).await;
        };
        let ending = match settled {
            ReconcileOutcome::Finished {
                succeeded: true, ..
            } => succeeded(shared, &row, held),
            ReconcileOutcome::Finished {
                succeeded: false,
                detail,
            } => {
                let reason = detail.unwrap_or_else(|| "the run failed".to_owned());
                Ending::new(operation, step, StepOutcome::Failed)
                    .because(reason)
                    .holding(held)
            }
            ReconcileOutcome::Unknown(reason) => unknown(operation, step, reason, held),
            ReconcileOutcome::NeverRan | ReconcileOutcome::HandoffFailed => {
                Ending::new(operation, step, StepOutcome::NotRun)
                    .because(NOT_RUN_REASON)
                    .holding(held)
            }
        };
        outcome(settle(shared, ending).await)
    }
}

/// A run the journal says worked: it owes the verification its skill asks for, read from the
/// skills the live session announces.
fn succeeded(shared: &OpShared, row: &ExecutionRecord, held: Held) -> Ending {
    let mut ending = Ending::new(row.operation, row.step, StepOutcome::Succeeded).holding(held);
    let session = shared.deps.observer.observe_snapshot(row.host);
    let skill = row.contract.skill_name().and_then(|name| {
        session
            .as_ref()
            .and_then(|live| live.skills.iter().find(|skill| skill.name == name))
    });
    ending.verify_after = verify_delay(&shared.settings, skill, row);
    ending
}

/// The execution row of `step` while the step is dispatched and running, read after the
/// host's dispatch gate so a run that was committed but not yet handed over is never read as
/// one the agent lost.
async fn running_row(
    shared: &OpShared,
    operation: OperationId,
    step: StepId,
) -> Option<ExecutionRecord> {
    let first = shared.deps.operations.load(operation).await.ok()??;
    let host = first.execution_of(step)?.value.host;
    drop(shared.dispatch_gate(host).await);
    let snapshot = load(shared, operation).await.ok()?;
    let record = snapshot.step(step)?;
    if record.step.kind != StepKind::Execution || record.progress.outcome != StepOutcome::Running {
        return None;
    }
    snapshot
        .execution_of(step)
        .map(|stored| stored.value.clone())
}

/// An execution whose outcome nobody can establish.
fn unknown(operation: OperationId, step: StepId, reason: UnknownReason, held: Held) -> Ending {
    Ending::new(operation, step, StepOutcome::OutcomeUnknown)
        .because(reason_text(reason))
        .holding(held)
}

/// Asks again later: the run is still going, or the agent cannot answer yet.
async fn again(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    step: StepId,
) -> EffectOutcome {
    let next = spec(
        EffectKind::Reconcile,
        step_payload(operation, step),
        after(shared, shared.settings.reconcile_interval),
        RECONCILE_ATTEMPTS,
        Some(reconcile_key(step)),
    );
    defer(shared, held, operation, None, next, false).await
}
