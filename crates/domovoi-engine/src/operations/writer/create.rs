//! Creating an operation: the row, its first steps, its incident links and its seed effects
//! in one commit, queued until admission lets it start.

use domovoi_core::ports::{
    AggregateRef, AuditDraft, EffectDraft, JournalDraft, JournalKind, OperationWrite, StateWrite,
    Transaction,
};
use domovoi_core::{Budgets, EffectId, Operation, OperationId, OperationSnapshot};

use super::OpShared;
use super::commit::{UuidIds, commit_error, push_decision};
use crate::operations::audit;
use crate::operations::decide::{DecideError, OperationEvent, decide};
use crate::operations::error::OperationError;
use crate::operations::handle::OperationSeed;
use crate::operations::journal;
use crate::operations::state::OperationState;

/// Attempts allowed to an effect a seed asks for.
const SEED_ATTEMPTS: u32 = 3;

/// Stores a new queued operation from `seed` and returns its id.
pub(super) async fn create(
    shared: &OpShared,
    seed: OperationSeed,
) -> Result<OperationId, OperationError> {
    let now = shared.deps.clock.now();
    let OperationSeed {
        kind,
        goal,
        pinned,
        incidents,
        parent,
        budgets,
        first_steps,
        effects,
        actor,
    } = seed;
    let limits = budgets.unwrap_or_else(|| shared.settings.budgets.clone());
    let operation = Operation::queued(
        OperationId::new(),
        kind,
        &goal,
        pinned,
        parent,
        Budgets::new(limits),
        now,
    )
    .map_err(|err| DecideError::Invalid(err.to_string()))?;
    let id = operation.id;
    let state = OperationState::new(OperationSnapshot {
        operation: operation.clone(),
        version: 0,
        steps: Vec::new(),
        edges: Vec::new(),
        executions: Vec::new(),
        incidents: incidents.clone(),
    });
    let decision = if first_steps.is_empty() {
        None
    } else {
        let event = OperationEvent::Propose {
            steps: first_steps,
            edges: Vec::new(),
        };
        Some(decide(&state, event, now, &mut UuidIds)?)
    };
    let stored = decision
        .as_ref()
        .map_or(operation.clone(), |d| d.operation.clone());
    let names: Vec<String> = incidents.iter().map(ToString::to_string).collect();
    let mut tx = Transaction::new()
        .write(StateWrite::Operation(OperationWrite::Operation {
            operation: stored,
            expected: domovoi_core::ports::Expected::New,
        }))
        .journal(JournalDraft {
            aggregate: AggregateRef::operation(id),
            kind: JournalKind::OperationCreated,
            payload_v: 1,
            payload: journal::operation_created(&operation, &names),
            actor: actor.clone(),
            occurred_at: now,
        })
        .audit(AuditDraft::new(
            actor.clone(),
            now,
            audit::OPERATION_CREATED.to_owned(),
            format!("operation:{id}"),
            serde_json::json!({ "operation": id.to_string(), "step": null }),
        ));
    if let Some(decision) = &decision {
        tx = push_decision(tx, decision, &[], &actor, now);
    }
    for incident in incidents {
        tx = tx.write(StateWrite::Operation(OperationWrite::IncidentLink {
            operation: id,
            incident,
        }));
    }
    for (kind, payload) in effects {
        tx = tx.effect(EffectDraft {
            id: EffectId::new(),
            kind,
            aggregate: AggregateRef::operation(id),
            payload,
            not_before: now,
            max_attempts: SEED_ATTEMPTS,
            dedup_key: None,
            created_at: now,
        });
    }
    let created = !tx.new_effects.is_empty();
    shared.deps.uow.commit(tx).await.map_err(commit_error)?;
    if created {
        shared.nudge.notify_one();
    }
    Ok(id)
}
