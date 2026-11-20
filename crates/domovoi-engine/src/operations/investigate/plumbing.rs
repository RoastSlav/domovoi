//! What the four handlers share: reading steps, ending the operation and the one writer
//! command that ends an effect together with a step and the effects that follow it.

use std::sync::Arc;

use domovoi_core::ports::{ClaimToken, EffectRecord};
use domovoi_core::{
    FinishOutcome, Incident, OperationId, OperationSnapshot, StepId, StepKind, StepRecord,
};

use super::Runtime;
use crate::effects::EffectOutcome;
use crate::operations::OperationError;
use crate::operations::decide::{EffectSpec, RecordedStep};
use crate::operations::effects::{ask, finish, held, outcome};
use crate::operations::handle::OperationHandle;
use crate::operations::writer::{Conclusion, Held, OpShared, OperationCommand, TurnEnd};

/// The investigation runtime of the service, which the handlers are only registered with.
pub(super) fn runtime(shared: &OpShared) -> Option<Arc<Runtime>> {
    shared.investigate.clone()
}

/// A handle over the same writer, to propose steps from a handler.
pub(super) fn handle_of(shared: &Arc<OpShared>) -> OperationHandle {
    OperationHandle {
        shared: shared.clone(),
    }
}

/// The claim a handler holds on `effect`.
pub(super) fn claim(effect: &EffectRecord, token: ClaimToken) -> Held {
    held(effect, token)
}

/// Ends the held effect, ends `step` when given and creates `successors`, in one commit.
pub(super) async fn conclude(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    step: Option<Conclusion>,
    successors: Vec<EffectSpec>,
) -> EffectOutcome {
    outcome(
        ask(shared, |reply| OperationCommand::Conclude {
            held,
            operation,
            step,
            successors,
            reply,
        })
        .await,
    )
}

/// Ends the held effect, ends the model turn `turn` with the reads it made as evidence steps,
/// and creates `successors`, in one commit.
pub(super) async fn finish_turn(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    turn: Conclusion,
    evidence: Vec<RecordedStep>,
    successors: Vec<EffectSpec>,
) -> EffectOutcome {
    let end = Box::new(TurnEnd {
        held,
        operation,
        turn,
        evidence,
        successors,
    });
    outcome(ask(shared, |reply| OperationCommand::FinishTurn { end, reply }).await)
}

/// Ends the operation and the held effect: there is nothing left to investigate. Finishing
/// closes any open step, so a turn that was running ends skipped.
pub(super) async fn end_operation(
    shared: &Arc<OpShared>,
    held: Held,
    operation: OperationId,
    result: FinishOutcome,
    summary: &str,
) -> EffectOutcome {
    match handle_of(shared)
        .finish(operation, result, summary.to_owned())
        .await
    {
        // A transient failure leaves the effect to be tried again: it is the only thing
        // that can still end this investigation.
        Err(
            err @ (OperationError::Store(_)
            | OperationError::Conflict
            | OperationError::Unavailable),
        ) => EffectOutcome::Retry {
            error: err.to_string(),
        },
        // Ended, or ended before: nothing is left for the effect to do.
        _ => finish(shared, held).await,
    }
}

/// The step `id` of the snapshot.
pub(super) fn step_of(snapshot: &OperationSnapshot, id: StepId) -> Option<&StepRecord> {
    snapshot.step(id)
}

/// The reasoning turn with the highest ordinal, optionally of one `cycle`.
pub(super) fn latest_turn(snapshot: &OperationSnapshot, cycle: Option<u32>) -> Option<&StepRecord> {
    snapshot
        .steps
        .iter()
        .filter(|record| record.step.kind == StepKind::ReasoningTurn)
        .filter(|record| cycle.is_none_or(|cycle| record.step.cycle == cycle))
        .max_by_key(|record| record.step.ordinal)
}

/// The incident the operation is about, or `None` when it no longer exists. A store error is
/// the handler's to retry.
pub(super) async fn incident_of(
    runtime: &Runtime,
    snapshot: &OperationSnapshot,
) -> Result<Option<Incident>, EffectOutcome> {
    let Some(id) = snapshot.incidents.first() else {
        return Ok(None);
    };
    runtime
        .triage
        .deps
        .incidents
        .get(*id)
        .await
        .map_err(|err| EffectOutcome::Retry {
            error: err.to_string(),
        })
}

/// A summary or reason cut to what a step stores.
pub(super) fn short(text: &str) -> String {
    cut(text, 280)
}

/// `text` cut to at most `max` characters.
pub(super) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
