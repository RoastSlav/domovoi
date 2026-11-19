//! A handler that ends its effect together with a step and the effects that follow.

use domovoi_core::OperationId;

use super::commands::{Conclusion, Held, Reply};
use super::commit::{finish_held_many, without_held_key};
use super::{OpShared, answer, held_change};
use crate::operations::decide::{EffectSpec, OperationEvent};

/// Redacts a step's output. Output that is JSON is redacted key by key and value by value: running the
/// redactor over the serialized text could let a secret swallow the quotes and keys next to
/// it and leave a document nothing can read back.
fn redact_output(shared: &OpShared, text: String) -> String {
    fn walk(value: &mut serde_json::Value, redact: &dyn Fn(&str) -> String) {
        match value {
            serde_json::Value::String(inner) => *inner = redact(inner),
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| walk(v, redact)),
            serde_json::Value::Object(map) => {
                // Keys are text an untrusted tool chose, so they are redacted too. Two keys
                // that redact to the same text keep the first and number the later ones.
                let mut clean = serde_json::Map::new();
                for (key, mut inner) in std::mem::take(map) {
                    walk(&mut inner, redact);
                    let mut name = redact(&key);
                    let mut n = 1;
                    while clean.contains_key(&name) {
                        n += 1;
                        name = format!("{}#{n}", redact(&key));
                    }
                    clean.insert(name, inner);
                }
                *map = clean;
            }
            _ => {}
        }
    }
    let redactor = &shared.deps.redactor;
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut value) => {
            walk(&mut value, &|inner| redactor.redact(inner));
            value.to_string()
        }
        Err(_) => redactor.redact(&text),
    }
}

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
        output: step.output.map(|text| redact_output(shared, text)),
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
