//! Contracts for the operation aggregate: its writes through the unit of work and its
//! repository reads, run against the in-memory store and SQLite.

mod reads;
mod structure;
mod writes;

use std::collections::BTreeMap;

use domovoi_core::ports::{CommitError, CommitReceipt, OperationWrite, StateWrite, Transaction};
use domovoi_core::{
    Actor, Budgets, CheckKey, ContractId, EdgeKind, ExecutionIntent, ExecutionRecord, HostId,
    Operation, OperationId, OperationKind, PinnedSubject, PlanHash, Step, StepEdge, StepId,
    StepIntent, StepProgress, TargetKey,
};

use super::{ContractError, UowSubject};
use crate::build::at;

pub use reads::{
    dispatched_on_lists_only_running_executions_of_the_host,
    dispatched_since_counts_by_host_contract_and_time,
    execution_round_trips_a_human_approval_exactly,
    operation_listing_filters_and_pages_newest_first,
    snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions,
    with_status_returns_oldest_first,
};
pub use structure::{
    edge_across_operations_is_refused, edge_to_a_later_or_equal_ordinal_is_refused,
    edge_to_a_missing_step_is_refused, execution_id_equal_to_an_action_id_is_refused,
    execution_row_needs_an_execution_step_and_is_one_to_one,
    execution_version_guard_refuses_a_stale_write, incident_link_is_idempotent,
    step_ordinals_are_dense_and_unique_within_an_operation,
};
pub use writes::{
    failed_operation_commit_rolls_back_every_stage,
    operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together,
    operation_new_over_existing_id_conflicts,
    operation_stale_version_conflicts_and_changes_nothing,
    step_progress_is_guarded_by_the_stored_outcome,
};

pub(in crate::contracts) fn setup(error: impl std::fmt::Display) -> ContractError {
    ContractError::Setup(error.to_string())
}

/// A queued operation pinned to one container check on `host`, created at second `secs`.
pub(in crate::contracts) fn operation(host: HostId, secs: i64) -> Result<Operation, ContractError> {
    Operation::queued(
        OperationId::new(),
        OperationKind::Investigate,
        "find out why plex is down",
        vec![PinnedSubject {
            host,
            check: CheckKey::parse("container.down").map_err(setup)?,
            target: Some(TargetKey::parse("plex").map_err(setup)?),
        }],
        None,
        Budgets::default(),
        at(secs),
    )
    .map_err(setup)
}

/// A step of `operation` at `ordinal` with `intent`.
fn step(operation: OperationId, ordinal: u32, intent: StepIntent, parent: Option<StepId>) -> Step {
    Step {
        id: StepId::new(),
        operation,
        ordinal,
        kind: intent.kind(),
        phase: "investigate".to_owned(),
        parent,
        cycle: 0,
        intent,
        created_at: at(10 + i64::from(ordinal)),
    }
}

/// A note step: the simplest step that needs nothing else.
fn note(operation: OperationId, ordinal: u32, parent: Option<StepId>) -> Step {
    step(
        operation,
        ordinal,
        StepIntent::Note {
            text: format!("note {ordinal}"),
            author: Actor::System("test".to_owned()),
        },
        parent,
    )
}

/// What a restart of a container asks for on `host`.
fn execution_intent(host: HostId) -> Result<ExecutionIntent, ContractError> {
    Ok(ExecutionIntent {
        contract: ContractId::parse("skill.restart_container").map_err(setup)?,
        contract_version: 1,
        host,
        check: CheckKey::parse("container.down").map_err(setup)?,
        target: Some(TargetKey::parse("plex").map_err(setup)?),
        args: BTreeMap::from([("container".to_owned(), "plex".to_owned())]),
        retry_of: None,
    })
}

/// An Execution step of `operation` at `ordinal`, with its proposed (unapproved) row.
pub(in crate::contracts) fn execution(
    operation: OperationId,
    ordinal: u32,
    host: HostId,
) -> Result<(Step, ExecutionRecord), ContractError> {
    let intent = execution_intent(host)?;
    let step = step(
        operation,
        ordinal,
        StepIntent::Execution(intent.clone()),
        None,
    );
    let hash: PlanHash = "ab".repeat(32).parse().map_err(setup)?;
    let record = ExecutionRecord::propose(
        &intent,
        step.id,
        operation,
        hash,
        at(10_000),
        domovoi_core::Risk::Write,
        at(10 + i64::from(ordinal)),
    );
    Ok((step, record))
}

/// A progress that marks a step running from second `secs`.
fn running(secs: i64) -> StepProgress {
    StepProgress {
        outcome: domovoi_core::StepOutcome::Running,
        started_at: Some(at(secs)),
        ..StepProgress::pending()
    }
}

/// Commits `writes`, in order, in one transaction.
pub(in crate::contracts) async fn commit(
    subject: &dyn UowSubject,
    writes: Vec<OperationWrite>,
) -> Result<CommitReceipt, CommitError> {
    let tx = writes.into_iter().fold(Transaction::new(), |tx, write| {
        tx.write(StateWrite::Operation(write))
    });
    subject.uow().commit(tx).await
}

/// Stores `operation` and `steps` (in order) and fails the contract if the store refuses.
pub(in crate::contracts) async fn seed(
    subject: &dyn UowSubject,
    operation: &Operation,
    steps: &[Step],
) -> Result<(), ContractError> {
    let mut writes = vec![OperationWrite::Operation {
        operation: operation.clone(),
        expected: domovoi_core::ports::Expected::New,
    }];
    writes.extend(steps.iter().cloned().map(OperationWrite::Step));
    commit(subject, writes).await?;
    Ok(())
}

/// Whether `outcome` is a refusal of a structural rule rather than a version conflict.
fn is_constraint(outcome: &Result<CommitReceipt, CommitError>) -> bool {
    matches!(outcome, Err(CommitError::Constraint(_)))
}

/// A `depends_on` edge from `from` to `to` in `operation`.
fn edge(operation: OperationId, from: StepId, to: StepId) -> StepEdge {
    StepEdge {
        operation,
        from,
        to,
        kind: EdgeKind::DependsOn,
    }
}
