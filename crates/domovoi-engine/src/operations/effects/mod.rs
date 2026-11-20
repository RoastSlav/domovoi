//! The effect handlers of the operation runner.
//!
//! This is the one place handlers are registered. A handler reads the step and the execution
//! it is about from the store, never from the payload (the payload is only the address), asks
//! the writer for the change, and reports `Handled` once the writer answered. Which effect
//! kinds have a handler is the whole list in [`register`]: nothing else the operation
//! aggregate creates is ever claimed.

mod dispatch;
mod evidence;
mod reconcile;
mod verify;
mod wake;

use std::sync::Arc;

use domovoi_core::ports::{AggregateKind, ClaimToken, EffectKind, EffectRecord, EffectStatus};
use domovoi_core::{EffectId, OperationId, OperationSnapshot, StepId, StepOutcome};
use jiff::{SignedDuration, Timestamp};
use serde_json::Value;
use tokio::sync::oneshot;

use super::decide::EffectSpec;
use super::error::OperationError;
use super::writer::{Held, OpShared, OperationCommand, Reply};
use crate::effects::{EffectOutcome, EffectRunner};

/// Registers a handler on `runner` for each effect kind the operation service claims.
pub(super) fn register(runner: &mut EffectRunner, shared: &Arc<OpShared>) {
    runner.register(
        EffectKind::CollectEvidence,
        Arc::new(evidence::CollectEvidence::new(shared.clone())),
    );
    runner.register(
        EffectKind::Dispatch,
        Arc::new(dispatch::DispatchHandler::new(shared.clone())),
    );
    runner.register(
        EffectKind::Wake,
        Arc::new(wake::WakeHandler::new(shared.clone())),
    );
    runner.register(
        EffectKind::Verify,
        Arc::new(verify::VerifyHandler::new(shared.clone())),
    );
    runner.register(
        EffectKind::DraftResolution,
        Arc::new(crate::operations::resolution::DraftResolutionHandler::new(
            shared.clone(),
        )),
    );
    runner.register(
        EffectKind::Reconcile,
        Arc::new(reconcile::ReconcileHandler::new(shared.clone())),
    );
}

/// What an effect payload names: the operation, and the step when the effect is about one.
pub(super) struct Address {
    /// The operation.
    pub(super) operation: OperationId,
    /// The step, when the payload names one.
    pub(super) step: Option<StepId>,
}

/// The address of `effect`, or `None` when the effect cannot be trusted to be about one
/// operation: its aggregate is not an operation, its payload is not version 1, or the
/// operation the payload names is not the effect's aggregate. This is the one place that is
/// checked, so a handler never touches an operation an effect is not addressed to.
pub(super) fn address(effect: &EffectRecord) -> Option<Address> {
    let payload = &effect.payload;
    if effect.aggregate.kind != AggregateKind::Operation || payload.get("v")?.as_u64()? != 1 {
        return None;
    }
    let operation: OperationId = payload.get("operation")?.as_str()?.parse().ok()?;
    if effect.aggregate.id.parse::<OperationId>().ok()? != operation {
        return None;
    }
    let step = match payload.get("step") {
        Some(step) => Some(step.as_str()?.parse().ok()?),
        None => None,
    };
    Some(Address { operation, step })
}

/// The outcome for a payload no handler can read.
pub(super) fn unreadable() -> EffectOutcome {
    EffectOutcome::Dead {
        error: "unreadable payload".to_owned(),
    }
}

/// The claim a handler holds on `effect`.
pub(super) fn held(effect: &EffectRecord, claim: ClaimToken) -> Held {
    Held {
        effect: effect.id,
        claim,
    }
}

/// The operation's snapshot, or the outcome the handler should end with: a missing operation
/// is a permanent failure, a store error is retried.
pub(super) async fn load(
    shared: &OpShared,
    operation: OperationId,
) -> Result<OperationSnapshot, EffectOutcome> {
    match shared.deps.operations.load(operation).await {
        Ok(Some(snapshot)) => Ok(snapshot),
        Ok(None) => Err(EffectOutcome::Dead {
            error: "the operation no longer exists".to_owned(),
        }),
        Err(err) => Err(EffectOutcome::Retry {
            error: err.to_string(),
        }),
    }
}

/// Sends a handler command to the writer and waits for its answer.
pub(super) async fn ask<T>(
    shared: &OpShared,
    make: impl FnOnce(Reply<T>) -> OperationCommand,
) -> Result<T, OperationError> {
    let (reply, answer) = oneshot::channel();
    if shared.writer.send(make(reply)).await.is_err() {
        return Err(OperationError::Unavailable);
    }
    answer.await.map_err(|_| OperationError::Unavailable)?
}

/// What a handler reports once the writer answered `result`.
///
/// The writer completes or reschedules the held effect itself when it refuses a change, and a
/// stale claim means someone else holds the effect, so those are `Handled`; a store failure or
/// a conflict is worth another attempt. A closing service leaves the effect claimed for the
/// next start.
pub(super) fn outcome(result: Result<(), OperationError>) -> EffectOutcome {
    match result {
        Err(OperationError::Store(error)) => EffectOutcome::Retry { error },
        Err(OperationError::Conflict) => EffectOutcome::Retry {
            error: "the operation changed meanwhile".to_owned(),
        },
        _ => EffectOutcome::Handled,
    }
}

/// Completes the held effect: it has nothing left to do.
pub(super) async fn finish(shared: &OpShared, held: Held) -> EffectOutcome {
    outcome(
        ask(shared, |reply| OperationCommand::FinishEffect {
            held,
            reply,
        })
        .await,
    )
}

/// How an execution ends, as the writer is asked to record it.
pub(super) struct Ending {
    /// The effect completed in the same commit, when the ending answers one.
    pub(super) held: Option<Held>,
    /// The operation.
    pub(super) operation: OperationId,
    /// The Execution step.
    pub(super) step: StepId,
    /// How it ended.
    pub(super) outcome: StepOutcome,
    /// Why it ended so.
    pub(super) reason: Option<String>,
    /// What the run printed.
    pub(super) output: Option<String>,
    /// How long to wait before re-checking, for a success that is verified.
    pub(super) verify_after: Option<SignedDuration>,
}

impl Ending {
    /// An ending with no reason, no output, no verification and no held effect.
    pub(super) fn new(operation: OperationId, step: StepId, outcome: StepOutcome) -> Self {
        Self {
            held: None,
            operation,
            step,
            outcome,
            reason: None,
            output: None,
            verify_after: None,
        }
    }

    /// The ending with `reason`.
    pub(super) fn because(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The ending that also completes `held`.
    pub(super) fn holding(mut self, held: Held) -> Self {
        self.held = Some(held);
        self
    }
}

/// Ends an execution through the writer.
pub(super) async fn settle(shared: &OpShared, ending: Ending) -> Result<(), OperationError> {
    let Ending {
        held,
        operation,
        step,
        outcome,
        reason,
        output,
        verify_after,
    } = ending;
    ask(shared, |reply| OperationCommand::Settle {
        held,
        operation,
        step,
        outcome,
        reason,
        output,
        verify_after,
        reply,
    })
    .await
}

/// Completes the held effect and creates `successor` in one commit; with `waiting` the
/// operation also records that it now waits for the step's agent.
pub(super) async fn defer(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    step: Option<StepId>,
    successor: EffectSpec,
    waiting: bool,
) -> EffectOutcome {
    let result = ask(shared, |reply| OperationCommand::Defer {
        held,
        operation,
        step,
        successor: Box::new(successor),
        waiting,
        reply,
    })
    .await;
    outcome(result)
}

/// A new effect to create, due at `not_before`.
pub(super) fn spec(
    kind: EffectKind,
    payload: Value,
    not_before: Timestamp,
    max_attempts: u32,
    dedup_key: Option<String>,
) -> EffectSpec {
    EffectSpec {
        id: EffectId::new(),
        kind,
        payload,
        not_before,
        max_attempts,
        dedup_key,
    }
}

/// The time `by` after now.
pub(super) fn after(shared: &OpShared, by: SignedDuration) -> Timestamp {
    let now = shared.deps.clock.now();
    now.checked_add(by).unwrap_or(now)
}

/// Whether `effect` is still claimed under `claim`: a handler that failed without finishing
/// the effect is retried, one whose effect the writer already completed is not.
pub(super) async fn still_held(shared: &OpShared, held: Held) -> bool {
    match shared.deps.effect_read.effect(held.effect).await {
        Ok(Some(record)) => {
            record.status == EffectStatus::Claimed.as_str()
                && record.claim_token == Some(held.claim)
        }
        _ => false,
    }
}
