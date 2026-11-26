//! Operation contracts about structure: ordinals, edges, execution rows and incident links.

use domovoi_core::ports::{
    AggregateRef, CommitError, ExecutionGuard, Expectation, Observed, OperationWrite,
};
use domovoi_core::{ExecutionId, HostId, IncidentId, OperationId};

use super::super::{ContractError, ContractResult, UowSubject};
use super::{commit, edge, execution, is_constraint, note, operation, seed};
use crate::build::at;

const EDGE_RULE: &str = "an edge must point to an earlier step of the same operation";

/// A new step takes the next ordinal of its operation and nothing else.
pub async fn step_ordinals_are_dense_and_unique_within_an_operation(
    subject: &dyn UowSubject,
) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    let first = note(op.id, 0, None);
    seed(subject, &op, std::slice::from_ref(&first)).await?;

    for bad in [
        note(op.id, 2, None),
        note(op.id, 0, None),
        note(op.id, 1, Some(domovoi_core::StepId::new())),
        note(OperationId::new(), 0, None),
    ] {
        let outcome = commit(subject, vec![OperationWrite::Step(bad)]).await;
        assert!(is_constraint(&outcome), "{outcome:?}");
    }
    let mut same_id = note(op.id, 1, None);
    same_id.id = first.id;
    assert!(is_constraint(
        &commit(subject, vec![OperationWrite::Step(same_id)]).await
    ));
    let mut empty_phase = note(op.id, 1, None);
    empty_phase.phase = String::new();
    assert!(is_constraint(
        &commit(subject, vec![OperationWrite::Step(empty_phase)]).await
    ));

    let second = note(op.id, 1, Some(first.id));
    commit(subject, vec![OperationWrite::Step(second.clone())]).await?;
    let snapshot = subject.operations().load(op.id).await?;
    let snapshot =
        snapshot.ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    let ordinals: Vec<u32> = snapshot.steps.iter().map(|r| r.step.ordinal).collect();
    assert_eq!(ordinals, vec![0, 1]);
    assert_eq!(snapshot.steps[1].step.parent, Some(first.id));
    Ok(())
}

/// An edge must point at a strictly lower ordinal; the other direction, a self-loop and a
/// repeat are refused.
pub async fn edge_to_a_later_or_equal_ordinal_is_refused(
    subject: &dyn UowSubject,
) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    let (a, b) = (note(op.id, 0, None), note(op.id, 1, None));
    seed(subject, &op, &[a.clone(), b.clone()]).await?;

    for (from, to) in [(a.id, b.id), (b.id, b.id)] {
        let outcome = commit(subject, vec![OperationWrite::Edge(edge(op.id, from, to))]).await;
        assert_eq!(outcome, Err(CommitError::Constraint(EDGE_RULE.to_owned())));
    }
    commit(subject, vec![OperationWrite::Edge(edge(op.id, b.id, a.id))]).await?;
    let repeat = commit(subject, vec![OperationWrite::Edge(edge(op.id, b.id, a.id))]).await;
    assert!(is_constraint(&repeat));
    Ok(())
}

/// An edge joins steps of one operation only.
pub async fn edge_across_operations_is_refused(subject: &dyn UowSubject) -> ContractResult {
    let (one, two) = (operation(HostId::new(), 1)?, operation(HostId::new(), 2)?);
    let (a0, a1) = (note(one.id, 0, None), note(one.id, 1, None));
    let b0 = note(two.id, 0, None);
    seed(subject, &one, &[a0.clone(), a1.clone()]).await?;
    seed(subject, &two, std::slice::from_ref(&b0)).await?;

    let across = commit(
        subject,
        vec![OperationWrite::Edge(edge(one.id, a1.id, b0.id))],
    )
    .await;
    assert_eq!(across, Err(CommitError::Constraint(EDGE_RULE.to_owned())));
    let mislabelled = commit(
        subject,
        vec![OperationWrite::Edge(edge(two.id, a1.id, a0.id))],
    )
    .await;
    assert_eq!(
        mislabelled,
        Err(CommitError::Constraint(EDGE_RULE.to_owned()))
    );
    Ok(())
}

/// An edge needs both of its steps to exist.
pub async fn edge_to_a_missing_step_is_refused(subject: &dyn UowSubject) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    let first = note(op.id, 0, None);
    seed(subject, &op, std::slice::from_ref(&first)).await?;
    let ghost = domovoi_core::StepId::new();
    for (from, to) in [(first.id, ghost), (ghost, first.id)] {
        let outcome = commit(subject, vec![OperationWrite::Edge(edge(op.id, from, to))]).await;
        assert!(is_constraint(&outcome), "{outcome:?}");
    }
    Ok(())
}

/// An execution row belongs to one Execution step of its own operation, once.
pub async fn execution_row_needs_an_execution_step_and_is_one_to_one(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let other = operation(host, 2)?;
    let plain = note(op.id, 0, None);
    let (run, row) = execution(op.id, 1, host)?;
    seed(subject, &op, &[plain.clone(), run.clone()]).await?;
    seed(subject, &other, &[]).await?;
    let write = |execution| OperationWrite::Execution {
        execution,
        expected: ExecutionGuard::New,
    };

    let (_, mut on_note) = execution(op.id, 0, host)?;
    on_note.step = plain.id;
    on_note.execution = ExecutionId::from_step(plain.id);
    let mut on_ghost = row.clone();
    on_ghost.step = domovoi_core::StepId::new();
    on_ghost.execution = ExecutionId::from_step(on_ghost.step);
    let mut elsewhere = row.clone();
    elsewhere.operation = other.id;
    let mut mismatched = row.clone();
    mismatched.execution = ExecutionId::new();
    for bad in [on_note, on_ghost, elsewhere, mismatched] {
        let outcome = commit(subject, vec![write(bad)]).await;
        assert!(is_constraint(&outcome), "{outcome:?}");
    }

    commit(subject, vec![write(row.clone())]).await?;
    let second = commit(subject, vec![write(row.clone())]).await;
    assert_eq!(
        second,
        Err(CommitError::Conflict {
            aggregate: AggregateRef::operation(op.id),
            expected: Expectation::New,
            found: Observed::Version(1),
        })
    );
    let snapshot = subject.operations().load(op.id).await?;
    let snapshot =
        snapshot.ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    assert_eq!(snapshot.executions.len(), 1);
    Ok(())
}

/// An execution write names the version it read; a stale one conflicts.
pub async fn execution_version_guard_refuses_a_stale_write(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let (run, row) = execution(op.id, 0, host)?;
    seed(subject, &op, std::slice::from_ref(&run)).await?;
    commit(
        subject,
        vec![OperationWrite::Execution {
            execution: row.clone(),
            expected: ExecutionGuard::New,
        }],
    )
    .await?;

    let mut waiting = row.clone();
    waiting.waiting_since = Some(at(40));
    waiting.updated_at = at(40);
    let update = |expected| OperationWrite::Execution {
        execution: waiting.clone(),
        expected,
    };
    commit(subject, vec![update(ExecutionGuard::Version(1))]).await?;
    let stale = commit(subject, vec![update(ExecutionGuard::Version(1))]).await;
    assert_eq!(
        stale,
        Err(CommitError::Conflict {
            aggregate: AggregateRef::operation(op.id),
            expected: Expectation::Version(1),
            found: Observed::Version(2),
        })
    );
    let stored = subject
        .operations()
        .execution(ExecutionId::from_step(run.id))
        .await?
        .ok_or_else(|| ContractError::Setup("the execution is missing".to_owned()))?;
    assert_eq!((stored.value, stored.version), (waiting.clone(), 2));

    let ghost = commit(
        subject,
        vec![OperationWrite::Execution {
            execution: waiting,
            expected: ExecutionGuard::Version(1),
        }],
    )
    .await;
    assert!(matches!(ghost, Err(CommitError::Conflict { .. })));
    Ok(())
}

/// Linking the same incident twice leaves one link; an unknown operation is refused.
pub async fn incident_link_is_idempotent(subject: &dyn UowSubject) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    seed(subject, &op, &[]).await?;
    let (one, two) = (IncidentId::new(), IncidentId::new());
    let link = |incident| OperationWrite::IncidentLink {
        operation: op.id,
        incident,
    };
    commit(subject, vec![link(two), link(one)]).await?;
    commit(subject, vec![link(one)]).await?;

    let mut expected = vec![one, two];
    expected.sort_by_key(ToString::to_string);
    let snapshot = subject.operations().load(op.id).await?;
    let snapshot =
        snapshot.ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    assert_eq!(snapshot.incidents, expected);
    assert_eq!(subject.operations().for_incident(one).await?, vec![op]);

    let unknown = commit(
        subject,
        vec![OperationWrite::IncidentLink {
            operation: OperationId::new(),
            incident: one,
        }],
    )
    .await;
    assert!(is_constraint(&unknown));
    Ok(())
}
