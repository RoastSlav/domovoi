//! A handler that ends its effect together with a step and the effects that follow.

use domovoi_core::OperationId;

use super::commands::{Conclusion, Held, Reply};
use super::commit::{finish_held_many, without_held_key};
use super::{OpShared, answer, held_change};
use crate::operations::decide::{EffectSpec, OperationEvent};

/// Completes `held`, ends `step` when given and creates `successors`, in one commit. A
/// refused step change completes the held effect without creating the successors.
pub(super) async fn run(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    step: Option<Conclusion>,
    successors: Vec<EffectSpec>,
    reply: Reply<()>,
) {
    let Some(step) = step else {
        let result = finish_held_many(shared, held, operation, successors).await;
        let _ = reply.send(result);
        return;
    };
    let redact = |text: String| shared.deps.redactor.redact(&text);
    let event = OperationEvent::StepFinished {
        step: step.step,
        outcome: step.outcome,
        summary: step.summary.map(redact),
        reason: step.reason.map(redact),
        output: step.output.map(redact),
        usage: step.usage,
    };
    let mut change = held_change(held, operation, event);
    for spec in successors {
        match without_held_key(shared, held, spec).await {
            Ok(spec) => change.effects.push(spec),
            Err(err) => {
                let _ = reply.send(Err(err));
                return;
            }
        }
    }
    answer(shared, change, reply).await;
}
