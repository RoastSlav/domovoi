//! Writing and editing resolution summaries. A summary revision is not an operation event, so
//! `decide` is not called: the revisions, their journal rows, the audit row of an owner's
//! edit and the completion of a held effect commit together.

use domovoi_core::experience::{SummaryField, SummaryRevision, SummarySource};
use domovoi_core::ports::{
    AggregateRef, AuditDraft, CommitError, EffectDraft, EffectKind, JournalDraft, JournalKind,
    OperationWrite, StateWrite, SummaryFieldWrite, Transaction,
};
use domovoi_core::{Actor, EffectId, OperationId};
use jiff::Timestamp;
use serde_json::json;
use tokio::sync::oneshot;

use super::OpShared;
use super::commands::{Held, Reply};
use super::commit::{commit_error, commit_plain, completion};
use super::execution::system;
use crate::operations::audit::OPERATION_SUMMARY_EDITED;
use crate::operations::decide::{resolution_key, resolution_payload};
use crate::operations::error::OperationError;
use crate::operations::resolution::{
    EditPlan, EditSummaryError, EditSummaryField, EditedField, PlannedRevision, SummaryError,
    plan_edit,
};

/// Attempts allowed to draft a summary.
const ATTEMPTS: u32 = 5;

fn write_of(operation: OperationId, plan: &PlannedRevision, at: Timestamp) -> SummaryFieldWrite {
    SummaryFieldWrite {
        operation,
        field: plan.field,
        expected_revision: plan.expected,
        value: plan.value.clone(),
        source: plan.source,
        actor: plan.actor.clone(),
        at,
        tags: plan.tags.clone(),
    }
}

/// Adds the write and its journal row to `tx`. The journal payload never holds the value:
/// only which field changed, to which revision, by which source.
fn push_revision(
    tx: Transaction,
    operation: OperationId,
    plan: &PlannedRevision,
    actor: &Actor,
    at: Timestamp,
) -> Transaction {
    let payload = json!({
        "operation": operation.to_string(),
        "field": plan.field.as_str(),
        "revision": plan.expected + 1,
        "source": plan.source.as_str(),
    });
    tx.write(StateWrite::Operation(OperationWrite::SummaryField(
        write_of(operation, plan, at),
    )))
    .journal(JournalDraft {
        aggregate: AggregateRef::operation(operation),
        kind: JournalKind::OperationSummary,
        payload_v: 1,
        payload,
        actor: actor.clone(),
        occurred_at: at,
    })
}

/// Appends the planned revisions (code fields first, as planned) and completes the held
/// effect, in one commit.
pub(super) async fn write_resolution(
    shared: &OpShared,
    operation: OperationId,
    revisions: Vec<PlannedRevision>,
    held: Held,
    reply: Reply<()>,
) {
    let now = shared.deps.clock.now();
    let actor = system();
    let mut tx = Transaction::new();
    for plan in &revisions {
        tx = push_revision(tx, operation, plan, &actor, now);
    }
    tx = tx.complete(completion(held, now));
    let _ = reply.send(commit_plain(shared, tx).await);
}

/// An owner's edit of a narrative field, through the writer.
pub(super) async fn edit(
    shared: &OpShared,
    operation: OperationId,
    edit: EditSummaryField,
    actor: Actor,
    reply: oneshot::Sender<Result<EditedField, EditSummaryError>>,
) {
    let _ = reply.send(edit_inner(shared, operation, edit, actor).await);
}

fn newest_revision(revisions: &[SummaryRevision], field: SummaryField) -> u32 {
    revisions
        .iter()
        .filter(|rev| rev.field == field)
        .map(|rev| rev.revision)
        .max()
        .unwrap_or(0)
}

async fn edit_inner(
    shared: &OpShared,
    operation: OperationId,
    edit: EditSummaryField,
    actor: Actor,
) -> Result<EditedField, EditSummaryError> {
    if edit.field.is_code_owned() {
        return Err(SummaryError::CodeOwned(edit.field).into());
    }
    let deps = &shared.deps;
    let store = |err: domovoi_core::ports::StoreError| OperationError::Store(err.to_string());
    let stored = deps.operations.get(operation).await.map_err(store)?;
    let terminal = stored
        .ok_or(OperationError::NotFound(operation))?
        .value
        .status
        .is_terminal();
    let existing = deps.experience.current(operation).await.map_err(store)?;
    let plan = match plan_edit(terminal, &existing, &edit, &actor, deps.redactor.as_ref()) {
        Ok(plan) => plan,
        Err(err) => return Err(err.into()),
    };
    let planned = match plan {
        EditPlan::Unchanged(revision) => {
            return Ok(EditedField {
                revision,
                unchanged: true,
            });
        }
        EditPlan::Write(planned) => planned,
    };
    let now = deps.clock.now();
    let mut tx = push_revision(Transaction::new(), operation, &planned, &actor, now);
    if planned.source == SummarySource::User {
        tx = tx.audit(AuditDraft::new(
            actor.clone(),
            now,
            OPERATION_SUMMARY_EDITED.to_owned(),
            format!("operation:{operation}"),
            json!({
                "operation": operation.to_string(),
                "field": planned.field.as_str(),
                "revision": planned.expected + 1,
            }),
        ));
    }
    match deps.uow.commit(tx).await {
        Ok(_) => {}
        Err(CommitError::Conflict { .. }) => {
            let fresh = deps.experience.current(operation).await.map_err(store)?;
            let actual = newest_revision(&fresh, edit.field);
            return Err(SummaryError::Stale {
                expected: edit.expected_revision,
                actual,
            }
            .into());
        }
        Err(err) => return Err(commit_error(err).into()),
    }
    Ok(EditedField {
        revision: SummaryRevision {
            operation,
            field: planned.field,
            revision: planned.expected + 1,
            value: planned.value,
            source: planned.source,
            actor: planned.actor,
            created_at: now,
        },
        unchanged: false,
    })
}

/// Queues the summary of a finished operation; one already queued is not an error.
pub(crate) async fn queue_resolution(
    shared: &OpShared,
    operation: OperationId,
) -> Result<(), OperationError> {
    let now = shared.deps.clock.now();
    let tx = Transaction::new().effect(EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::DraftResolution,
        aggregate: AggregateRef::operation(operation),
        payload: resolution_payload(operation),
        not_before: now,
        max_attempts: ATTEMPTS,
        dedup_key: Some(resolution_key(operation)),
        created_at: now,
    });
    match shared.deps.uow.commit(tx).await {
        Ok(_) => {
            shared.nudge.notify_one();
            Ok(())
        }
        Err(CommitError::Constraint(message)) if message.contains("duplicate effect") => Ok(()),
        Err(err) => Err(commit_error(err)),
    }
}
