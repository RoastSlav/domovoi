//! Operation contracts about reads: round trips, snapshots and the repository's queries.

use domovoi_core::ports::{ExecutionGuard, Expected, OperationFilter, OperationWrite, Page};
use domovoi_core::{
    ApprovalChannel, ApprovalProvenance, ContractId, EdgeKind, ExecutionId, HostId, HumanApproval,
    IncidentId, JournalEpoch, OperationId, OperationKind, OperationStatus, PauseReason,
    PlanBinding, StepEdge, StepId, StepOutcome, UserId,
};
use serde_json::json;

use super::super::{ContractError, ContractResult, UowSubject};
use super::{commit, execution, note, operation, running, seed};
use crate::approvals::authorize;
use crate::build::at;

fn epoch(n: u128) -> Result<JournalEpoch, ContractError> {
    serde_json::from_value(json!(uuid_text(n))).map_err(super::setup)
}

fn uuid_text(n: u128) -> String {
    format!("0190a0a0-0000-7000-8000-{n:012x}")
}

fn new_execution(row: domovoi_core::ExecutionRecord) -> OperationWrite {
    OperationWrite::Execution {
        execution: row,
        expected: ExecutionGuard::New,
    }
}

async fn snapshot(
    subject: &dyn UowSubject,
    id: OperationId,
) -> Result<domovoi_core::OperationSnapshot, ContractError> {
    subject
        .operations()
        .load(id)
        .await?
        .ok_or_else(|| ContractError::Setup("the operation is missing".to_owned()))
}

/// A human approval, its provenance, the dispatch epoch and every timestamp come back from
/// the store exactly as written, for a chat and a web approval.
pub async fn execution_round_trips_a_human_approval_exactly(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let (chat_step, mut chat) = execution(op.id, 0, host)?;
    let (web_step, mut web) = execution(op.id, 1, host)?;
    seed(subject, &op, &[chat_step.clone(), web_step.clone()]).await?;

    let chat_approval = HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Chat {
            platform: "discord".to_owned(),
            platform_user: "u-17".to_owned(),
        },
        at(50),
    );
    let binding = PlanBinding::Full(chat.plan_hash);
    chat.authorize(
        authorize(chat_approval.clone()),
        ApprovalProvenance::new(Some("session-ref-1".to_owned()), "chat:discord"),
        &binding,
        at(60),
    )
    .map_err(super::setup)?;
    chat.waiting_since = Some(at(61));
    chat.dispatched = Some((epoch(7)?, at(70)));
    chat.settled_at = Some(at(80));

    let web_approval = HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Web {
            typed_confirmation: true,
        },
        at(51),
    );
    let binding = PlanBinding::Prefix(web.plan_hash.as_bytes()[..8].to_vec());
    web.authorize(
        authorize(web_approval),
        ApprovalProvenance::new(None, "web"),
        &binding,
        at(62),
    )
    .map_err(super::setup)?;

    commit(
        subject,
        vec![new_execution(chat.clone()), new_execution(web.clone())],
    )
    .await?;

    let stored = subject
        .operations()
        .execution(ExecutionId::from_step(chat_step.id))
        .await?
        .ok_or_else(|| ContractError::Setup("the execution is missing".to_owned()))?;
    assert_eq!(stored.value, chat);
    assert_eq!(stored.version, 1);
    let approval = stored.value.approval();
    assert_eq!(approval.map(|a| a.approval()), Some(&chat_approval));
    assert_eq!(approval.map(|a| a.displayed()), Some(chat.plan_hash));
    assert_eq!(
        approval.and_then(|a| a.provenance().session_ref()),
        Some("session-ref-1")
    );
    assert_eq!(stored.value.dispatched, Some((epoch(7)?, at(70))));

    let loaded = snapshot(subject, op.id).await?;
    assert_eq!(loaded.executions.len(), 2);
    assert_eq!(loaded.executions[0].value, chat);
    assert_eq!(loaded.executions[1].value, web);

    let plain = subject.operations().execution(ExecutionId::new()).await?;
    assert_eq!(plain, None);
    Ok(())
}

/// A snapshot lists steps by ordinal, edges in the order they were written, executions by
/// their step's ordinal and the linked incidents, all from the one read.
pub async fn snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    let (first_run, first_row) = execution(op.id, 1, host)?;
    let (second_run, second_row) = execution(op.id, 3, host)?;
    let (zero, two) = (note(op.id, 0, None), note(op.id, 2, None));
    seed(
        subject,
        &op,
        &[
            zero.clone(),
            first_run.clone(),
            two.clone(),
            second_run.clone(),
        ],
    )
    .await?;
    let edge = |from: StepId, to: StepId| {
        OperationWrite::Edge(StepEdge {
            operation: op.id,
            from,
            to,
            kind: EdgeKind::CausedBy,
        })
    };
    let incident = IncidentId::new();
    commit(
        subject,
        vec![
            new_execution(second_row.clone()),
            new_execution(first_row.clone()),
            edge(second_run.id, two.id),
            edge(first_run.id, zero.id),
            edge(two.id, first_run.id),
            OperationWrite::IncidentLink {
                operation: op.id,
                incident,
            },
        ],
    )
    .await?;

    let loaded = snapshot(subject, op.id).await?;
    let ordinals: Vec<u32> = loaded.steps.iter().map(|r| r.step.ordinal).collect();
    assert_eq!(ordinals, vec![0, 1, 2, 3]);
    let edges: Vec<(StepId, StepId)> = loaded.edges.iter().map(|e| (e.from, e.to)).collect();
    assert_eq!(
        edges,
        vec![
            (second_run.id, two.id),
            (first_run.id, zero.id),
            (two.id, first_run.id)
        ]
    );
    let rows: Vec<_> = loaded.executions.iter().map(|r| r.value.clone()).collect();
    assert_eq!(rows, vec![first_row, second_row]);
    assert_eq!(loaded.incidents, vec![incident]);
    assert_eq!(loaded.version, 1);
    assert_eq!(subject.operations().load(OperationId::new()).await?, None);
    assert_eq!(subject.operations().get(OperationId::new()).await?, None);
    Ok(())
}

/// Only executions whose step is running and whose host is asked about are listed.
pub async fn dispatched_on_lists_only_running_executions_of_the_host(
    subject: &dyn UowSubject,
) -> ContractResult {
    let (here, there) = (HostId::new(), HostId::new());
    let op = operation(here, 1)?;
    let (a, row_a) = execution(op.id, 0, here)?;
    let (b, row_b) = execution(op.id, 1, here)?;
    let (c, row_c) = execution(op.id, 2, here)?;
    let (d, row_d) = execution(op.id, 3, there)?;
    seed(subject, &op, &[a.clone(), b.clone(), c.clone(), d.clone()]).await?;
    let mut writes: Vec<OperationWrite> = [&row_a, &row_b, &row_c, &row_d]
        .into_iter()
        .map(|row| new_execution(row.clone()))
        .collect();
    for step in [&a, &c, &d] {
        writes.push(OperationWrite::Progress {
            step: step.id,
            from: StepOutcome::Pending,
            to: running(20),
        });
    }
    writes.push(OperationWrite::Progress {
        step: c.id,
        from: StepOutcome::Running,
        to: domovoi_core::StepProgress {
            outcome: StepOutcome::Succeeded,
            started_at: Some(at(20)),
            ended_at: Some(at(25)),
            ..domovoi_core::StepProgress::pending()
        },
    });
    commit(subject, writes).await?;

    assert_eq!(subject.operations().dispatched_on(here).await?, vec![row_a]);
    assert_eq!(
        subject.operations().dispatched_on(there).await?,
        vec![row_d]
    );
    assert!(
        subject
            .operations()
            .dispatched_on(HostId::new())
            .await?
            .is_empty()
    );
    Ok(())
}

/// The count includes executions from the given time on, for one host and contract, and
/// ignores how they ended.
pub async fn dispatched_since_counts_by_host_contract_and_time(
    subject: &dyn UowSubject,
) -> ContractResult {
    let (here, there) = (HostId::new(), HostId::new());
    let op = operation(here, 1)?;
    let restart = ContractId::parse("skill.restart_container").map_err(super::setup)?;
    let prune = ContractId::parse("skill.prune_logs").map_err(super::setup)?;
    let mut steps = Vec::new();
    let mut writes = Vec::new();
    // (host, contract, dispatched at second, whether it was dispatched at all)
    let plan = [
        (here, &restart, 100, true),
        (here, &restart, 200, true),
        (here, &restart, 300, true),
        (here, &prune, 250, true),
        (there, &restart, 250, true),
        (here, &restart, 0, false),
    ];
    for (ordinal, (host, contract, secs, sent)) in plan.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(super::setup)?;
        let (step, mut row) = execution(op.id, ordinal, host)?;
        row.contract = contract.clone();
        if sent {
            row.dispatched = Some((epoch(1)?, at(secs)));
        }
        steps.push(step);
        writes.push(new_execution(row));
    }
    seed(subject, &op, &steps).await?;
    commit(subject, writes).await?;

    let count = |host, contract: &ContractId, secs| {
        let repo = subject.operations();
        let contract = contract.clone();
        async move { repo.dispatched_since(host, &contract, at(secs)).await }
    };
    assert_eq!(count(here, &restart, 150).await?, 2);
    assert_eq!(count(here, &restart, 100).await?, 3, "the boundary counts");
    assert_eq!(count(here, &restart, 301).await?, 0);
    assert_eq!(count(here, &prune, 0).await?, 1);
    assert_eq!(count(there, &restart, 0).await?, 1);
    assert_eq!(count(there, &prune, 0).await?, 0);
    Ok(())
}

/// Operations come back oldest first, filtered by status name, and a paused one keeps its
/// reason.
pub async fn with_status_returns_oldest_first(subject: &dyn UowSubject) -> ContractResult {
    let host = HostId::new();
    let (first, second, third, fourth) = (
        operation(host, 1)?,
        operation(host, 2)?,
        operation(host, 3)?,
        operation(host, 4)?,
    );
    // Created out of order so the store's own ordering decides.
    for op in [&third, &first, &fourth, &second] {
        seed(subject, op, &[]).await?;
    }
    let mut running_op = second.clone();
    running_op.status = OperationStatus::Running;
    running_op.started_at = Some(at(5));
    running_op.updated_at = at(5);
    let mut paused = fourth.clone();
    paused.status = OperationStatus::Paused {
        reason: PauseReason::GlobalPause,
    };
    paused.updated_at = at(6);
    commit(
        subject,
        vec![
            OperationWrite::Operation {
                operation: running_op.clone(),
                expected: Expected::Version(1),
            },
            OperationWrite::Operation {
                operation: paused.clone(),
                expected: Expected::Version(1),
            },
        ],
    )
    .await?;

    let repo = subject.operations();
    assert_eq!(
        repo.with_status(&["queued"], 10).await?,
        vec![first.clone(), third]
    );
    assert_eq!(repo.with_status(&["queued"], 1).await?, vec![first]);
    assert_eq!(
        repo.with_status(&["paused", "running"], 10).await?,
        vec![running_op, paused]
    );
    assert!(repo.with_status(&[], 10).await?.is_empty());
    assert!(repo.with_status(&["cancelled"], 10).await?.is_empty());
    Ok(())
}

/// A listing is newest first, pages by cursor and filters by status, incident, host and kind.
pub async fn operation_listing_filters_and_pages_newest_first(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let (first, second, third) = (
        operation(host, 1)?,
        operation(host, 2)?,
        operation(host, 3)?,
    );
    for op in [&first, &second, &third] {
        seed(subject, op, &[]).await?;
    }
    let incident = IncidentId::new();
    commit(
        subject,
        vec![OperationWrite::IncidentLink {
            operation: second.id,
            incident,
        }],
    )
    .await?;

    let repo = subject.operations();
    let all = OperationFilter::default();
    let page = repo.list(&all, &Page::first(10)).await?;
    assert_eq!(
        page.items,
        vec![third.clone(), second.clone(), first.clone()]
    );
    assert_eq!(page.next_cursor, None);

    let head = repo.list(&all, &Page::first(2)).await?;
    assert_eq!(head.items, vec![third, second.clone()]);
    let cursor = head.next_cursor.clone();
    assert!(cursor.is_some());
    let tail = repo.list(&all, &Page { cursor, limit: 2 }).await?;
    assert_eq!(tail.items, vec![first]);
    assert_eq!(tail.next_cursor, None);

    let by_incident = OperationFilter {
        incident: Some(incident),
        ..OperationFilter::default()
    };
    assert_eq!(
        repo.list(&by_incident, &Page::first(10)).await?.items,
        vec![second.clone()]
    );
    assert_eq!(repo.for_incident(incident).await?, vec![second]);
    assert!(repo.for_incident(IncidentId::new()).await?.is_empty());

    let elsewhere = OperationFilter {
        host: Some(HostId::new()),
        ..OperationFilter::default()
    };
    assert!(
        repo.list(&elsewhere, &Page::first(10))
            .await?
            .items
            .is_empty()
    );
    let here = OperationFilter {
        host: Some(host),
        status: Some("queued".to_owned()),
        kind: Some(OperationKind::Investigate),
        ..OperationFilter::default()
    };
    assert_eq!(repo.list(&here, &Page::first(10)).await?.items.len(), 3);
    let running_only = OperationFilter {
        status: Some("running".to_owned()),
        ..OperationFilter::default()
    };
    assert!(
        repo.list(&running_only, &Page::first(10))
            .await?
            .items
            .is_empty()
    );
    Ok(())
}
