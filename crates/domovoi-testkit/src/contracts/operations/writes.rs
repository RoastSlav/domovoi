//! Operation contracts about writes: versions, steps, edges, execution rows and rollback.

use domovoi_core::ports::{
    AggregateRef, AuditDraft, AuditFilter, ClaimToken, CommitError, EffectCompletion, EffectDraft,
    EffectKind, ExecutionGuard, Expectation, Expected, JournalDraft, JournalKind, Observed,
    OperationWrite, Page, StateWrite, Transaction,
};
use domovoi_core::{Actor, EffectId, HostId, IncidentId, OperationId, StepOutcome, StepProgress};
use serde_json::json;

use super::super::{ContractError, ContractResult, UowSubject};
use super::{commit, edge, execution, is_constraint, note, operation, running, seed};
use crate::build::at;

fn effect_of(aggregate: &AggregateRef, secs: i64) -> EffectDraft {
    EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Wake,
        aggregate: aggregate.clone(),
        payload: json!({"v": 1}),
        not_before: at(secs),
        max_attempts: 3,
        dedup_key: None,
        created_at: at(secs),
    }
}

fn journal_of(aggregate: &AggregateRef, secs: i64) -> JournalDraft {
    JournalDraft {
        aggregate: aggregate.clone(),
        kind: JournalKind::OperationCreated,
        payload_v: 1,
        payload: json!({"kind": "investigate"}),
        actor: Actor::System("test".to_owned()),
        occurred_at: at(secs),
    }
}

fn audit_of(subject: &str, secs: i64) -> AuditDraft {
    AuditDraft::new(
        Actor::System("test".to_owned()),
        at(secs),
        "operation.created".to_owned(),
        subject.to_owned(),
        serde_json::Value::Null,
    )
}

/// An operation, its steps, an edge, an execution row, an incident link, a journal entry,
/// an audit row and an effect commit together and read back as one snapshot.
pub async fn operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let first = note(op.id, 0, None);
    let second = note(op.id, 1, Some(first.id));
    let (run, row) = execution(op.id, 2, host)?;
    let incident = IncidentId::new();
    let aggregate = AggregateRef::operation(op.id);
    let wake = effect_of(&aggregate, 20);

    let tx = Transaction::new()
        .write(StateWrite::Operation(OperationWrite::Operation {
            operation: op.clone(),
            expected: Expected::New,
        }))
        .write(StateWrite::Operation(OperationWrite::Step(first.clone())))
        .write(StateWrite::Operation(OperationWrite::Step(second.clone())))
        .write(StateWrite::Operation(OperationWrite::Step(run.clone())))
        .write(StateWrite::Operation(OperationWrite::Edge(edge(
            op.id, run.id, first.id,
        ))))
        .write(StateWrite::Operation(OperationWrite::Execution {
            execution: row.clone(),
            expected: ExecutionGuard::New,
        }))
        .write(StateWrite::Operation(OperationWrite::IncidentLink {
            operation: op.id,
            incident,
        }))
        .journal(journal_of(&aggregate, 1))
        .audit(audit_of(&format!("operation:{}", op.id), 1))
        .effect(wake.clone());
    let receipt = subject.uow().commit(tx).await?;
    assert_eq!(receipt.versions, vec![(aggregate.clone(), 1)]);
    assert_eq!(receipt.journal_seqs.len(), 1);
    assert_eq!(receipt.audit_seqs.len(), 1);

    let snapshot = subject
        .operations()
        .load(op.id)
        .await?
        .ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    assert_eq!(snapshot.operation, op);
    assert_eq!(snapshot.version, 1);
    let steps: Vec<_> = snapshot.steps.iter().map(|r| r.step.clone()).collect();
    assert_eq!(steps, vec![first.clone(), second, run.clone()]);
    assert!(
        snapshot
            .steps
            .iter()
            .all(|r| r.progress == StepProgress::pending())
    );
    assert_eq!(snapshot.edges, vec![edge(op.id, run.id, first.id)]);
    assert_eq!(snapshot.executions.len(), 1);
    assert_eq!(snapshot.executions[0].value, row);
    assert_eq!(snapshot.executions[0].version, 1);
    assert_eq!(snapshot.incidents, vec![incident]);

    let entries = subject.journal().entries_for(&aggregate, 10).await?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, "operation.created");
    let audit = subject
        .audit()
        .list(&AuditFilter::default(), &Page::first(10))
        .await?;
    assert_eq!(audit.items.len(), 1);
    let stored = subject.effects().effect(wake.id).await?;
    let stored = stored.ok_or_else(|| ContractError::Setup("the effect is missing".to_owned()))?;
    assert_eq!(stored.status, "pending");
    assert_eq!(stored.aggregate, aggregate);
    Ok(())
}

/// Creating an operation whose id is taken conflicts and reports the stored version.
pub async fn operation_new_over_existing_id_conflicts(subject: &dyn UowSubject) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    seed(subject, &op, &[]).await?;
    let again = commit(
        subject,
        vec![OperationWrite::Operation {
            operation: op.clone(),
            expected: Expected::New,
        }],
    )
    .await;
    assert_eq!(
        again,
        Err(CommitError::Conflict {
            aggregate: AggregateRef::operation(op.id),
            expected: Expectation::New,
            found: Observed::Version(1),
        })
    );
    Ok(())
}

/// A stale version conflicts and rolls back what the transaction wrote before it; a current
/// version replaces the row and bumps the version.
pub async fn operation_stale_version_conflicts_and_changes_nothing(
    subject: &dyn UowSubject,
) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    seed(subject, &op, &[]).await?;
    let aggregate = AggregateRef::operation(op.id);

    let mut changed = op.clone();
    changed.cycle = 1;
    changed.updated_at = at(5);
    let stale = commit(
        subject,
        vec![
            OperationWrite::Step(note(op.id, 0, None)),
            OperationWrite::Operation {
                operation: changed.clone(),
                expected: Expected::Version(2),
            },
        ],
    )
    .await;
    assert_eq!(
        stale,
        Err(CommitError::Conflict {
            aggregate: aggregate.clone(),
            expected: Expectation::Version(2),
            found: Observed::Version(1),
        })
    );
    let unchanged = subject.operations().load(op.id).await?;
    let unchanged =
        unchanged.ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    assert_eq!(unchanged.operation, op);
    assert!(unchanged.steps.is_empty(), "the step rolled back");

    let missing = commit(
        subject,
        vec![OperationWrite::Operation {
            operation: operation(HostId::new(), 2)?,
            expected: Expected::Version(1),
        }],
    )
    .await;
    assert!(matches!(
        missing,
        Err(CommitError::Conflict {
            found: Observed::Missing,
            ..
        })
    ));

    let receipt = commit(
        subject,
        vec![OperationWrite::Operation {
            operation: changed.clone(),
            expected: Expected::Version(1),
        }],
    )
    .await?;
    assert_eq!(receipt.versions, vec![(aggregate, 2)]);
    let stored = subject.operations().get(op.id).await?;
    let stored =
        stored.ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    assert_eq!((stored.value, stored.version), (changed, 2));
    Ok(())
}

/// A progress write applies only while the stored outcome is the one the caller read.
pub async fn step_progress_is_guarded_by_the_stored_outcome(
    subject: &dyn UowSubject,
) -> ContractResult {
    let op = operation(HostId::new(), 1)?;
    let first = note(op.id, 0, None);
    seed(subject, &op, std::slice::from_ref(&first)).await?;
    let to_running = |from| OperationWrite::Progress {
        step: first.id,
        from,
        to: running(20),
    };

    commit(subject, vec![to_running(StepOutcome::Pending)]).await?;
    let again = commit(subject, vec![to_running(StepOutcome::Pending)]).await;
    assert_eq!(
        again,
        Err(CommitError::Conflict {
            aggregate: AggregateRef::operation(op.id),
            expected: Expectation::Status("pending".to_owned()),
            found: Observed::Status("running".to_owned()),
        })
    );

    let done = StepProgress {
        outcome: StepOutcome::Failed,
        started_at: Some(at(20)),
        ended_at: Some(at(30)),
        summary: Some("it failed".to_owned()),
        reason: Some("exit code 1".to_owned()),
        output: Some("some output".to_owned()),
    };
    commit(
        subject,
        vec![OperationWrite::Progress {
            step: first.id,
            from: StepOutcome::Running,
            to: done.clone(),
        }],
    )
    .await?;
    let snapshot = subject.operations().load(op.id).await?;
    let snapshot =
        snapshot.ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))?;
    assert_eq!(snapshot.steps[0].step, first, "the intent never changes");
    assert_eq!(snapshot.steps[0].progress, done);

    let unknown = commit(
        subject,
        vec![OperationWrite::Progress {
            step: domovoi_core::StepId::new(),
            from: StepOutcome::Pending,
            to: running(1),
        }],
    )
    .await;
    assert!(is_constraint(&unknown));
    Ok(())
}

/// A failure at the last stage of a commit (a stale effect completion) takes back the
/// operation writes, the journal, the audit row and the new effect.
pub async fn failed_operation_commit_rolls_back_every_stage(
    subject: &dyn UowSubject,
) -> ContractResult {
    let aggregate = AggregateRef::operation(OperationId::new());
    let held = effect_of(&aggregate, 1);
    subject
        .uow()
        .commit(Transaction::new().effect(held.clone()))
        .await?;
    subject
        .force_claim(held.id, ClaimToken::new(), at(60))
        .await?;
    let before_effect = subject.effects().effect(held.id).await?;
    let before_head = subject.journal().head_seq().await?;
    let before_audit = subject
        .audit()
        .list(&AuditFilter::default(), &Page::first(50))
        .await?;

    let host = HostId::new();
    let op = operation(host, 2)?;
    let first = note(op.id, 0, None);
    let fresh = AggregateRef::operation(op.id);
    let new_effect = effect_of(&fresh, 3);
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Operation(OperationWrite::Operation {
                    operation: op.clone(),
                    expected: Expected::New,
                }))
                .write(StateWrite::Operation(OperationWrite::Step(first)))
                .journal(journal_of(&fresh, 3))
                .audit(audit_of("operation:x", 3))
                .effect(new_effect.clone())
                .complete(EffectCompletion {
                    id: held.id,
                    claim_token: Some(ClaimToken::new()),
                    at: at(4),
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::StaleEffect { effect }) if effect == held.id
    ));

    assert_eq!(subject.operations().load(op.id).await?, None);
    assert_eq!(subject.journal().head_seq().await?, before_head);
    assert_eq!(
        subject
            .audit()
            .list(&AuditFilter::default(), &Page::first(50))
            .await?,
        before_audit
    );
    assert_eq!(subject.effects().effect(new_effect.id).await?, None);
    assert_eq!(subject.effects().effect(held.id).await?, before_effect);
    Ok(())
}
