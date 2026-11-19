//! Contracts for recorded shadow decisions: candidates, idempotent recording, filtered
//! listings and summaries, run against the in-memory store and SQLite.

use domovoi_core::policy_store::{
    PolicyWrite, ShadowFilter, ShadowRecord, ShadowStatus, SweepStats,
};
use domovoi_core::ports::{ExecutionGuard, OperationWrite, Page};
use domovoi_core::{ExecutionId, HostId, OperationId};

use super::operations::{commit, execution, operation, seed};
use super::policy::{HASH, write};
use super::{ContractResult, UowSubject};
use crate::build::at;

fn shadow(
    (execution, operation, host): (ExecutionId, OperationId, HostId),
    contract: &str,
    secs: i64,
    status: ShadowStatus,
    outcomes: (Option<&str>, Option<&str>),
    degraded: bool,
) -> ShadowRecord {
    ShadowRecord {
        execution,
        operation,
        host,
        contract: contract.to_owned(),
        step_created_at: at(secs - 1),
        evaluated_at: at(secs),
        status,
        reason: None,
        policy_version: 0,
        rules_hash: HASH.to_owned(),
        enforced_outcome: outcomes.0.map(str::to_owned),
        shadow_outcome: outcomes.1.map(str::to_owned),
        degraded,
        candidate: "{}".to_owned(),
        inputs: None,
        enforced: None,
        shadow: None,
        gaps: "[]".to_owned(),
    }
}

async fn record(subject: &dyn UowSubject, records: Vec<ShadowRecord>, secs: i64) -> ContractResult {
    let mut sweep = SweepStats::default();
    for record in &records {
        match record.status {
            ShadowStatus::Evaluated => sweep.evaluated += 1,
            ShadowStatus::Unevaluable => sweep.unevaluable += 1,
            ShadowStatus::Skipped => sweep.skipped += 1,
        }
    }
    write(
        subject,
        PolicyWrite::RecordShadow {
            records,
            sweep,
            at: at(secs),
            error: None,
        },
    )
    .await?;
    Ok(())
}

/// Candidates are the executions without a decision row, oldest first.
pub async fn shadow_candidates_are_unrecorded_executions_oldest_first(
    subject: &dyn UowSubject,
) -> ContractResult {
    let host = HostId::new();
    let op = operation(host, 1)?;
    seed(subject, &op, &[]).await?;
    let mut ids = Vec::new();
    for ordinal in 0..3 {
        let (step, row) = execution(op.id, ordinal, host)?;
        commit(
            subject,
            vec![
                OperationWrite::Step(step),
                OperationWrite::Execution {
                    execution: row.clone(),
                    expected: ExecutionGuard::New,
                },
            ],
        )
        .await?;
        ids.push(row.execution);
    }
    let all = subject.policy().shadow_candidates(10).await?;
    assert_eq!(all.iter().map(|c| c.execution).collect::<Vec<_>>(), ids);
    assert_eq!(all[0].operation, op.id);
    assert_eq!(all[0].host, host);
    assert_eq!(all[0].pinned, op.pinned);

    let middle = shadow(
        (ids[1], op.id, host),
        "skill.restart_container",
        100,
        ShadowStatus::Evaluated,
        (Some("allow"), Some("allow")),
        false,
    );
    record(subject, vec![middle], 100).await?;
    let rest = subject.policy().shadow_candidates(10).await?;
    assert_eq!(
        rest.iter().map(|c| c.execution).collect::<Vec<_>>(),
        vec![ids[0], ids[2]]
    );
    assert_eq!(subject.policy().shadow_candidates(1).await?.len(), 1);
    Ok(())
}

/// A second row for the same execution is ignored and not counted.
pub async fn record_shadow_is_idempotent_and_counts_inserted_rows(
    subject: &dyn UowSubject,
) -> ContractResult {
    let (exec, op, host) = (ExecutionId::new(), OperationId::new(), HostId::new());
    let first = shadow(
        (exec, op, host),
        "skill.a",
        100,
        ShadowStatus::Evaluated,
        (Some("allow"), Some("allow")),
        false,
    );
    let again = shadow(
        (exec, op, host),
        "skill.a",
        200,
        ShadowStatus::Evaluated,
        (Some("deny"), Some("deny")),
        true,
    );
    record(subject, vec![first.clone()], 100).await?;
    record(subject, vec![again], 200).await?;
    assert_eq!(subject.policy().shadow(exec).await?, Some(first));
    let state = subject.policy().shadow_state().await?;
    assert_eq!(state.total_evaluated, 1);
    assert_eq!(state.last_sweep_at, Some(at(200)));

    write(
        subject,
        PolicyWrite::RecordShadow {
            records: vec![],
            sweep: SweepStats::default(),
            at: at(300),
            error: Some("stored rules unreadable".to_owned()),
        },
    )
    .await?;
    let state = subject.policy().shadow_state().await?;
    assert_eq!(state.last_error.as_deref(), Some("stored rules unreadable"));
    assert_eq!(state.last_sweep_at, Some(at(200)));
    record(subject, vec![], 400).await?;
    assert_eq!(subject.policy().shadow_state().await?.last_error, None);
    Ok(())
}

/// Seeds five rows across two hosts and statuses; returns the hosts and ids in time order.
async fn seed_mix(
    subject: &dyn UowSubject,
) -> Result<(HostId, HostId, Vec<ExecutionId>), crate::ContractError> {
    let (a, b, op) = (HostId::new(), HostId::new(), OperationId::new());
    let ids: Vec<ExecutionId> = (0..5).map(|_| ExecutionId::new()).collect();
    let rows = vec![
        shadow(
            (ids[0], op, a),
            "skill.x",
            10,
            ShadowStatus::Evaluated,
            (Some("allow"), Some("allow")),
            false,
        ),
        shadow(
            (ids[1], op, a),
            "skill.x",
            20,
            ShadowStatus::Evaluated,
            (Some("require_approval"), Some("allow")),
            true,
        ),
        shadow(
            (ids[2], op, b),
            "skill.y",
            30,
            ShadowStatus::Unevaluable,
            (None, None),
            true,
        ),
        shadow(
            (ids[3], op, b),
            "skill.y",
            40,
            ShadowStatus::Evaluated,
            (Some("deny"), Some("deny")),
            false,
        ),
        shadow(
            (ids[4], op, a),
            "skill.x",
            50,
            ShadowStatus::Skipped,
            (None, None),
            false,
        ),
    ];
    record(subject, rows, 60).await?;
    Ok((a, b, ids))
}

/// Listings filter by host, status, difference and window, and page newest first.
pub async fn shadow_records_filter_and_page_newest_first(
    subject: &dyn UowSubject,
) -> ContractResult {
    let (a, _b, ids) = seed_mix(subject).await?;
    let list = |filter: ShadowFilter| async move {
        subject
            .policy()
            .shadow_records(&filter, &Page::first(50))
            .await
            .map(|page| page.items.iter().map(|r| r.execution).collect::<Vec<_>>())
    };
    let by_host = list(ShadowFilter {
        host: Some(a),
        ..ShadowFilter::default()
    })
    .await?;
    assert_eq!(by_host, vec![ids[4], ids[1], ids[0]]);
    let by_status = list(ShadowFilter {
        status: Some(ShadowStatus::Unevaluable),
        ..ShadowFilter::default()
    })
    .await?;
    assert_eq!(by_status, vec![ids[2]]);
    let differing = list(ShadowFilter {
        differing: true,
        ..ShadowFilter::default()
    })
    .await?;
    assert_eq!(differing, vec![ids[1]]);
    let windowed = list(ShadowFilter {
        since: Some(at(20)),
        until: Some(at(50)),
        ..ShadowFilter::default()
    })
    .await?;
    assert_eq!(windowed, vec![ids[3], ids[2], ids[1]]);
    let outcome = list(ShadowFilter {
        enforced_outcome: Some("deny".to_owned()),
        ..ShadowFilter::default()
    })
    .await?;
    assert_eq!(outcome, vec![ids[3]]);
    let contract = list(ShadowFilter {
        contract: Some("skill.y".to_owned()),
        ..ShadowFilter::default()
    })
    .await?;
    assert_eq!(contract, vec![ids[3], ids[2]]);

    let mut seen = Vec::new();
    let mut page = Page::first(2);
    loop {
        let got = subject
            .policy()
            .shadow_records(&ShadowFilter::default(), &page)
            .await?;
        seen.extend(got.items.iter().map(|r| r.execution));
        match got.next_cursor {
            Some(cursor) => {
                page = Page {
                    cursor: Some(cursor),
                    limit: 2,
                }
            }
            None => break,
        }
    }
    let expected: Vec<_> = ids.iter().rev().copied().collect();
    assert_eq!(seen, expected);
    Ok(())
}

/// The summary counts rows by status and enforced outcome.
pub async fn shadow_summary_counts_by_status_and_outcome(
    subject: &dyn UowSubject,
) -> ContractResult {
    seed_mix(subject).await?;
    let summary = subject.policy().shadow_summary(at(20)).await?;
    assert_eq!(summary.total, 4);
    assert_eq!(summary.by_status.get("evaluated"), Some(&2));
    assert_eq!(summary.by_status.get("unevaluable"), Some(&1));
    assert_eq!(summary.by_status.get("skipped"), Some(&1));
    assert_eq!(summary.by_enforced_outcome.len(), 2);
    assert_eq!(
        summary.by_enforced_outcome.get("require_approval"),
        Some(&1)
    );
    assert_eq!(summary.by_enforced_outcome.get("deny"), Some(&1));
    assert_eq!(summary.degraded, 2);
    assert_eq!(summary.differing, 1);
    Ok(())
}

/// An unevaluable row with only one outcome recorded is not a difference, in the filter or the
/// summary; an evaluated row with two different outcomes is.
pub async fn unevaluable_rows_are_not_counted_as_differing(
    subject: &dyn UowSubject,
) -> ContractResult {
    let (host, op) = (HostId::new(), OperationId::new());
    let (partial, evaluated) = (ExecutionId::new(), ExecutionId::new());
    let rows = vec![
        shadow(
            (partial, op, host),
            "skill.x",
            10,
            ShadowStatus::Unevaluable,
            (Some("allow"), None),
            false,
        ),
        shadow(
            (evaluated, op, host),
            "skill.x",
            20,
            ShadowStatus::Evaluated,
            (Some("allow"), Some("deny")),
            false,
        ),
    ];
    record(subject, rows, 30).await?;
    let differing = subject
        .policy()
        .shadow_records(
            &ShadowFilter {
                differing: true,
                ..ShadowFilter::default()
            },
            &Page::first(10),
        )
        .await?;
    let seen: Vec<_> = differing.items.iter().map(|r| r.execution).collect();
    assert_eq!(seen, vec![evaluated]);
    assert_eq!(subject.policy().shadow_summary(at(0)).await?.differing, 1);
    Ok(())
}

/// A store with no policy answers every read with nothing.
pub async fn empty_store_reads_are_empty(subject: &dyn UowSubject) -> ContractResult {
    let repo = subject.policy();
    assert!(repo.active().await?.is_none());
    assert!(repo.draft().await?.is_none());
    assert!(repo.version(1).await?.is_none());
    assert!(repo.versions(&Page::first(10)).await?.items.is_empty());
    assert!(repo.shadow(ExecutionId::new()).await?.is_none());
    let none = repo
        .shadow_records(&ShadowFilter::default(), &Page::first(10))
        .await?;
    assert!(none.items.is_empty() && none.next_cursor.is_none());
    assert_eq!(
        repo.shadow_state().await?,
        domovoi_core::policy_store::ShadowState::default()
    );
    assert!(repo.shadow_candidates(10).await?.is_empty());
    Ok(())
}
