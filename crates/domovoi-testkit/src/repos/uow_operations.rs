//! Applying the operation aggregate's writes to in-memory state, with the same rules and the
//! same errors as the SQLite store.

use domovoi_core::ports::{
    AggregateRef, CommitError, CommitReceipt, ExecutionGuard, Expectation, Expected, Observed,
    OperationWrite,
};
use domovoi_core::{
    ActionId, ExecutionId, ExecutionRecord, IncidentId, Operation, OperationId, Step, StepEdge,
    StepId, StepKind, StepOutcome, StepProgress, StepRecord,
};

use super::{State, set_version, version};

/// The message every refused edge carries.
const EDGE_RULE: &str = "an edge must point to an earlier step of the same operation";

fn refuse(message: &str) -> CommitError {
    CommitError::Constraint(message.to_owned())
}

fn conflict(id: OperationId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::operation(id),
        expected,
        found,
    }
}

/// Where a stored step sits: its operation and its position in that operation's steps.
fn locate(state: &State, id: StepId) -> Option<(OperationId, usize)> {
    state.op_steps.iter().find_map(|(operation, steps)| {
        steps
            .iter()
            .position(|record| record.step.id == id)
            .map(|index| (*operation, index))
    })
}

/// Stores one operation write.
pub(super) fn apply(
    state: &mut State,
    write: &OperationWrite,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match write {
        OperationWrite::Operation {
            operation,
            expected,
        } => apply_operation(state, operation, *expected, receipt),
        OperationWrite::Step(step) => insert_step(state, step),
        OperationWrite::Progress { step, from, to } => update_progress(state, *step, *from, to),
        OperationWrite::Edge(edge) => insert_edge(state, edge),
        OperationWrite::Execution {
            execution,
            expected,
        } => apply_execution(state, execution, *expected),
        OperationWrite::IncidentLink {
            operation,
            incident,
        } => link_incident(state, *operation, *incident),
        OperationWrite::SummaryField(write) => super::experience::apply(state, write),
    }
}

fn apply_operation(
    state: &mut State,
    operation: &Operation,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    operation
        .validate()
        .map_err(|e| CommitError::Constraint(e.to_string()))?;
    let aggregate = AggregateRef::operation(operation.id);
    let exists = state.operations.contains_key(&operation.id);
    let stored = version(state, &aggregate);
    let next = match expected {
        Expected::New if exists => {
            return Err(conflict(
                operation.id,
                Expectation::New,
                Observed::Version(stored),
            ));
        }
        Expected::New => 1,
        Expected::Version(n) if !exists => {
            return Err(conflict(
                operation.id,
                Expectation::Version(n),
                Observed::Missing,
            ));
        }
        Expected::Version(n) if stored != n => {
            return Err(conflict(
                operation.id,
                Expectation::Version(n),
                Observed::Version(stored),
            ));
        }
        Expected::Version(n) => n + 1,
    };
    if let Some(parent) = operation.parent
        && !state.operations.contains_key(&parent)
    {
        return Err(refuse("FOREIGN KEY constraint failed"));
    }
    state.operations.insert(operation.id, operation.clone());
    set_version(state, aggregate.clone(), next);
    receipt.versions.push((aggregate, next));
    Ok(())
}

fn insert_step(state: &mut State, step: &Step) -> Result<(), CommitError> {
    step.validate()
        .map_err(|e| CommitError::Constraint(e.to_string()))?;
    if !state.operations.contains_key(&step.operation) {
        return Err(refuse("unknown operation"));
    }
    if locate(state, step.id).is_some() {
        return Err(refuse("step id exists"));
    }
    let count = state.op_steps.get(&step.operation).map_or(0, Vec::len);
    if usize::try_from(step.ordinal).ok() != Some(count) {
        return Err(refuse(
            "step ordinals are dense: a new step takes the next one",
        ));
    }
    if let Some(parent) = step.parent {
        let ok = locate(state, parent)
            .is_some_and(|(operation, index)| operation == step.operation && index < count);
        if !ok {
            return Err(refuse(
                "a step's parent must be an earlier step of the same operation",
            ));
        }
    }
    state
        .op_steps
        .entry(step.operation)
        .or_default()
        .push(StepRecord {
            step: step.clone(),
            progress: StepProgress::pending(),
        });
    Ok(())
}

fn update_progress(
    state: &mut State,
    step: StepId,
    from: StepOutcome,
    to: &StepProgress,
) -> Result<(), CommitError> {
    to.validate()
        .map_err(|e| CommitError::Constraint(e.to_string()))?;
    let Some((operation, index)) = locate(state, step) else {
        return Err(refuse("unknown step"));
    };
    let Some(record) = state
        .op_steps
        .get_mut(&operation)
        .and_then(|steps| steps.get_mut(index))
    else {
        return Err(refuse("unknown step"));
    };
    if record.progress.outcome != from {
        return Err(conflict(
            operation,
            Expectation::Status(from.as_str().to_owned()),
            Observed::Status(record.progress.outcome.as_str().to_owned()),
        ));
    }
    record.progress = to.clone();
    Ok(())
}

fn insert_edge(state: &mut State, edge: &StepEdge) -> Result<(), CommitError> {
    let (Some(from), Some(to)) = (locate(state, edge.from), locate(state, edge.to)) else {
        return Err(refuse("unknown step"));
    };
    if from.0 != edge.operation || to.0 != edge.operation || to.1 >= from.1 {
        return Err(refuse(EDGE_RULE));
    }
    let edges = state.op_edges.entry(edge.operation).or_default();
    if edges
        .iter()
        .any(|stored| (stored.from, stored.to, stored.kind) == (edge.from, edge.to, edge.kind))
    {
        return Err(refuse("edge exists"));
    }
    edges.push(*edge);
    Ok(())
}

fn apply_execution(
    state: &mut State,
    execution: &ExecutionRecord,
    expected: ExecutionGuard,
) -> Result<(), CommitError> {
    if execution.execution != ExecutionId::from_step(execution.step) {
        return Err(refuse("an execution is keyed by its step's id"));
    }
    let Some((operation, index)) = locate(state, execution.step) else {
        return Err(refuse("unknown step"));
    };
    let kind = state
        .op_steps
        .get(&operation)
        .and_then(|steps| steps.get(index))
        .map(|record| record.step.kind);
    if kind != Some(StepKind::Execution) {
        return Err(refuse("an execution row needs an Execution step"));
    }
    if operation != execution.operation {
        return Err(refuse(
            "the execution and its step belong to different operations",
        ));
    }
    if state
        .actions
        .contains_key(&ActionId::from_uuid(execution.step.as_uuid()))
    {
        return Err(refuse("execution id is an action id"));
    }
    let stored = state.executions.get(&execution.step).map(|(_, v)| *v);
    let next = match (expected, stored) {
        (ExecutionGuard::New, Some(version)) => {
            return Err(conflict(
                operation,
                Expectation::New,
                Observed::Version(version),
            ));
        }
        (ExecutionGuard::New, None) => 1,
        (ExecutionGuard::Version(n), None) => {
            return Err(conflict(
                operation,
                Expectation::Version(n),
                Observed::Missing,
            ));
        }
        (ExecutionGuard::Version(n), Some(version)) if version != n => {
            return Err(conflict(
                operation,
                Expectation::Version(n),
                Observed::Version(version),
            ));
        }
        (ExecutionGuard::Version(n), Some(_)) => n + 1,
    };
    state
        .executions
        .insert(execution.step, (execution.clone(), next));
    Ok(())
}

fn link_incident(
    state: &mut State,
    operation: OperationId,
    incident: IncidentId,
) -> Result<(), CommitError> {
    if !state.operations.contains_key(&operation) {
        return Err(refuse("unknown operation"));
    }
    state
        .op_incidents
        .entry(operation)
        .or_default()
        .insert(incident);
    Ok(())
}
