//! Contracts for permits and autonomy state: the permit row written with its execution, the
//! pause switch, and the queries the circuit breaker and the unknown-outcome guard read.

use domovoi_core::ports::{
    CommitError, ExecutionGuard, OperationWrite, PermitWrite, StateWrite, Transaction,
    UnknownResolution,
};
use domovoi_core::{
    Authorization, ExecutionId, HostId, PolicyPermit, PolicyPermitParts, StepOutcome, StepProgress,
};

use super::super::{ContractError, ContractResult, UowSubject};
use super::{commit, execution, operation, seed, setup};
use crate::build::at;

fn permit_auth(
    plan: domovoi_core::PlanHash,
    rules: &[&str],
    issued: i64,
) -> Result<Authorization, ContractError> {
    let permit = PolicyPermit::issue(PolicyPermitParts {
        decision_id: "decision-1",
        rule_ids: rules.iter().map(|r| (*r).to_owned()).collect(),
        policy_version: "3",
        plan_hash: plan,
        issued_at: at(issued),
        expires_at: at(issued + 300),
        ceiling_hash: &"cd".repeat(32),
        safety_identity: None,
    })
    .map_err(setup)?;
    Ok(Authorization::policy(permit))
}

fn outcome(outcome: StepOutcome, secs: i64) -> StepProgress {
    StepProgress {
        outcome,
        started_at: Some(at(secs)),
        ended_at: Some(at(secs + 1)),
        summary: None,
        reason: None,
        output: None,
    }
}

/// The permit is stored with its execution and comes back whole; a snapshot load has it too.
pub async fn permit_row_is_written_with_the_authorization_atomically(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let (step, mut row) = execution(op.id, 0, host)?;
    seed(subject, &op, std::slice::from_ref(&step)).await?;
    row.authorize_by_permit(
        &permit_auth(row.plan_hash, &["r-b", "r-a"], 20)?,
        "decision document".to_owned(),
        at(21),
    )
    .map_err(setup)?;
    commit(
        subject,
        vec![OperationWrite::Execution {
            execution: row.clone(),
            expected: ExecutionGuard::New,
        }],
    )
    .await?;

    let stored = subject
        .operations()
        .execution(ExecutionId::from_step(step.id))
        .await?
        .ok_or_else(|| ContractError::Setup("the execution is missing".to_owned()))?;
    assert_eq!(stored.value, row);
    assert_eq!(
        stored.value.authorization_kind(),
        Some(domovoi_core::AuthorizationKind::Policy)
    );
    let permit = stored
        .value
        .permit()
        .map(|p| p.permit().rule_ids().to_vec());
    assert_eq!(permit, Some(vec!["r-a".to_owned(), "r-b".to_owned()]));
    assert_eq!(
        stored.value.permit().map(|p| p.decision()),
        Some("decision document")
    );

    let snapshot = subject.operations().load(op.id).await?;
    let snapshot = snapshot.ok_or_else(|| ContractError::Setup("missing".to_owned()))?;
    assert_eq!(snapshot.executions[0].value, row);

    let all = subject.permits().permits_issued_since(at(0), None).await?;
    let scoped = subject
        .permits()
        .permits_issued_since(at(0), Some((host, &row.contract)))
        .await?;
    let later = subject
        .permits()
        .permits_issued_since(at(1000), None)
        .await?;
    assert_eq!((all, scoped, later), (1, 1, 0));
    assert_eq!(subject.permits().rule_permits_since("r-a", at(0)).await?, 1);
    assert_eq!(subject.permits().rule_permits_since("r-z", at(0)).await?, 0);
    Ok(())
}

/// The pause switch is one row: not paused at the start, paused and resumed by writes.
pub async fn autonomy_row_is_single_and_defaults_to_not_paused(
    subject: &dyn UowSubject,
) -> ContractResult {
    let start = subject.permits().autonomy().await?;
    assert!(!start.paused && start.changed_at.is_none());
    let pause = PermitWrite::Pause {
        by: "owner".to_owned(),
        reason: "maintenance".to_owned(),
        at: at(5),
    };
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Permit(pause)))
        .await?;
    let paused = subject.permits().autonomy().await?;
    assert!(paused.paused);
    assert_eq!(paused.reason.as_deref(), Some("maintenance"));
    assert_eq!(paused.changed_by.as_deref(), Some("owner"));
    let resume = PermitWrite::Resume {
        by: "owner".to_owned(),
        at: at(9),
    };
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Permit(resume)))
        .await?;
    let resumed = subject.permits().autonomy().await?;
    assert!(!resumed.paused && resumed.reason.is_none());
    assert_eq!(resumed.changed_at, Some(at(9)));
    Ok(())
}

/// Failures count by rule inside the window, an `OutcomeUnknown` counts, a refusal does not,
/// failures older than the window do not, and a reset row clears earlier failures.
pub async fn circuit_failures_query_counts_by_rule_and_window(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let mut rows = Vec::new();
    let mut steps = Vec::new();
    for ordinal in 0..4 {
        let (step, row) = execution(op.id, ordinal, host)?;
        steps.push(step);
        rows.push(row);
    }
    seed(subject, &op, &steps).await?;
    let settled = [100, 200, 300, 50];
    let outcomes = [
        StepOutcome::Failed,
        StepOutcome::OutcomeUnknown,
        StepOutcome::Refused,
        StepOutcome::Failed,
    ];
    let mut writes = Vec::new();
    for (i, row) in rows.iter_mut().enumerate() {
        row.authorize_by_permit(
            &permit_auth(row.plan_hash, &["r1"], 10)?,
            "d".to_owned(),
            at(11),
        )
        .map_err(setup)?;
        row.settled_at = Some(at(settled[i]));
        writes.push(OperationWrite::Execution {
            execution: row.clone(),
            expected: ExecutionGuard::New,
        });
    }
    for (i, step) in steps.iter().enumerate() {
        writes.push(OperationWrite::Progress {
            step: step.id,
            from: StepOutcome::Pending,
            to: outcome(outcomes[i], settled[i]),
        });
    }
    commit(subject, writes).await?;

    let inside = subject.permits().rule_failures(at(60)).await?;
    assert_eq!(inside.len(), 1);
    assert_eq!((inside[0].rule.as_str(), inside[0].failures), ("r1", 2));
    assert_eq!(inside[0].since, at(100));

    let wide = subject.permits().rule_failures(at(0)).await?;
    assert_eq!(
        wide[0].failures, 3,
        "the older failure counts in a wider window"
    );

    let reset = PermitWrite::ResetRule {
        rule: "r1".to_owned(),
        by: "owner".to_owned(),
        at: at(150),
    };
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Permit(reset)))
        .await?;
    let after = subject.permits().rule_failures(at(0)).await?;
    assert_eq!((after[0].rule.as_str(), after[0].failures), ("r1", 1));
    Ok(())
}

/// An unresolved unknown outcome is found by host, contract and target; resolving it once
/// clears it, and a second resolution conflicts.
pub async fn unknown_resolution_is_unique_per_execution(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let (step, row) = execution(op.id, 0, host)?;
    seed(subject, &op, std::slice::from_ref(&step)).await?;
    commit(
        subject,
        vec![
            OperationWrite::Execution {
                execution: row.clone(),
                expected: ExecutionGuard::New,
            },
            OperationWrite::Progress {
                step: step.id,
                from: StepOutcome::Pending,
                to: outcome(StepOutcome::OutcomeUnknown, 40),
            },
        ],
    )
    .await?;
    let target = row
        .target
        .clone()
        .ok_or_else(|| ContractError::Setup("no target".to_owned()))?;
    let found = subject
        .permits()
        .unresolved_unknown(host, &row.contract, &target)
        .await?;
    assert_eq!(found.map(|u| u.step), Some(step.id));
    let other_host = subject
        .permits()
        .unresolved_unknown(HostId::new(), &row.contract, &target)
        .await?;
    assert!(other_host.is_none());

    let resolve = || PermitWrite::ResolveUnknown {
        execution: row.execution,
        by: "owner".to_owned(),
        how: UnknownResolution::Owner,
        note: Some("checked the host".to_owned()),
        at: at(90),
    };
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Permit(resolve())))
        .await?;
    let cleared = subject
        .permits()
        .unresolved_unknown(host, &row.contract, &target)
        .await?;
    assert!(cleared.is_none());
    let twice = subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Permit(resolve())))
        .await;
    assert!(matches!(twice, Err(CommitError::Constraint(_))));
    Ok(())
}
