//! A handler that ends its effect together with a step and the effects that follow.

use domovoi_core::OperationId;

use super::commands::{Conclusion, Held, Reply, TurnEnd};
use super::commit::{finish_held_many, without_held_key};
use super::{OpShared, answer, held_change};
use crate::operations::decide::{EffectSpec, OperationEvent, RecordedStep};

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

/// Why a recorded read cannot be stored, `None` when it can.
fn unusable(read: &RecordedStep) -> Option<String> {
    use domovoi_core::{StepIntent, StepOutcome};
    if !matches!(
        read.outcome,
        StepOutcome::Succeeded
            | StepOutcome::Failed
            | StepOutcome::Unavailable
            | StepOutcome::Refused
    ) {
        return Some(format!("a read ended {}", read.outcome.as_str()));
    }
    StepIntent::EvidenceRequest {
        host: read.host,
        tool: read.tool.clone(),
        args: read.args.clone(),
    }
    .validate()
    .err()
    .map(|err| err.to_string())
}

/// Ends a model turn: its reads become evidence steps, the turn ends, the successors are
/// created and the held effect completes, in one commit. A read that does not validate would
/// refuse the whole decision, so the turn is committed without its reads and says so in its
/// reason: a report is never lost to a bad record.
pub(super) async fn run_turn(shared: &OpShared, end: TurnEnd, reply: Reply<()>) {
    let TurnEnd {
        held,
        operation,
        turn,
        mut evidence,
        successors,
    } = end;
    let mut reason = turn.reason;
    if let Some(problem) = evidence.iter().find_map(unusable) {
        tracing::error!(%operation, problem, "a recorded read does not validate; the turn is stored without its reads");
        evidence.clear();
        let note = "its reads were not recorded";
        reason = Some(reason.map_or_else(|| note.to_owned(), |old| format!("{old}; {note}")));
    }
    let redact = |text: String| shared.deps.redactor.redact(&text);
    let evidence = evidence
        .into_iter()
        .map(|mut read| {
            read.summary = read.summary.map(redact);
            read.reason = read.reason.map(redact);
            read.output = read.output.map(redact);
            read
        })
        .collect();
    let event = OperationEvent::TurnFinished {
        step: turn.step,
        outcome: turn.outcome,
        summary: turn.summary.map(redact),
        reason: reason.map(redact),
        output: turn.output.map(|text| redact_output(shared, text)),
        usage: turn.usage,
        evidence,
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
