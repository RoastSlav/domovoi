//! Containment blocks, in SQLite and in the in-memory twin: creation, guarded phase moves,
//! uniqueness of the confirm and rollback executions, the reconcile listing and retention.

use domovoi_core::ports::{
    BlockPhase, BlockRow, CommitError, ContainmentRepo, ContainmentWrite, Expectation, Observed,
    PhaseGuard, PhaseUpdate, StateWrite, Transaction, UnitOfWork, phase_journal,
};
use domovoi_core::{Actor, ExecutionId, HostId, OperationId, StepId};
use domovoi_store::PURGED_TABLES;
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::at;
use jiff::SignedDuration;

use crate::common::Fresh;

fn block(host: HostId, created: i64) -> BlockRow {
    BlockRow {
        block_id: ExecutionId::from_step(StepId::new()),
        host_id: host,
        operation_id: OperationId::new(),
        step_id: StepId::new(),
        source: "203.0.113.0/24".to_owned(),
        ttl_s: 600,
        plan_hash: "ab".repeat(32),
        phase: BlockPhase::Applying,
        authorization_kind: "human".to_owned(),
        assurance: r#"{"kind":"unverified","reason":"key_not_pinned","unprotected_paths":[]}"#
            .to_owned(),
        confirm_execution: None,
        rollback_execution: None,
        applied_at: None,
        deadline: None,
        existing_flows: None,
        bridge_gap: None,
        failure_token: None,
        failure_message: None,
        created_at: at(created),
        updated_at: at(created),
        version: 1,
    }
}

fn create(row: &BlockRow) -> Transaction {
    Transaction::new().write(StateWrite::Containment(ContainmentWrite::Create(
        row.clone(),
    )))
}

fn guard(row: &BlockRow) -> PhaseGuard {
    PhaseGuard {
        phase: row.phase,
        version: row.version,
    }
}

fn phase(row: &BlockRow, update: PhaseUpdate) -> Transaction {
    Transaction::new().write(StateWrite::Containment(ContainmentWrite::Phase {
        block: row.block_id,
        expected: guard(row),
        update,
    }))
}

async fn reread(store: &impl ContainmentRepo, row: &BlockRow) -> BlockRow {
    store.get_block(row.block_id).await.unwrap().unwrap()
}

async fn a_block_round_trips_and_phase_moves_keep_unchanged_fields(
    store: &(impl UnitOfWork + ContainmentRepo),
) {
    let host = HostId::new();
    let row = block(host, 1);
    store.commit(create(&row)).await.unwrap();
    assert_eq!(reread(store, &row).await, row);

    let confirm = ExecutionId::from_step(StepId::new());
    let mut applied = PhaseUpdate::to(BlockPhase::Applied, at(5));
    applied.applied_at = Some(at(5));
    applied.existing_flows = Some("n/a".to_owned());
    applied.bridge_gap = Some(false);
    store.commit(phase(&row, applied)).await.unwrap();
    let after_apply = reread(store, &row).await;
    assert_eq!(after_apply.phase, BlockPhase::Applied);
    assert_eq!(after_apply.version, 2);
    assert_eq!(after_apply.applied_at, Some(at(5)));
    assert_eq!(after_apply.updated_at, at(5));
    assert_eq!(after_apply.bridge_gap, Some(false));

    let mut confirming = PhaseUpdate::to(BlockPhase::Confirming, at(6));
    confirming.confirm_execution = Some(confirm);
    store.commit(phase(&after_apply, confirming)).await.unwrap();
    let mut confirmed = PhaseUpdate::to(BlockPhase::Confirmed, at(7));
    confirmed.deadline = Some(at(607));
    let after_confirming = reread(store, &row).await;
    store
        .commit(phase(&after_confirming, confirmed))
        .await
        .unwrap();
    let last = reread(store, &row).await;
    assert_eq!(last.phase, BlockPhase::Confirmed);
    assert_eq!(last.version, 4);
    assert_eq!(last.confirm_execution, Some(confirm));
    assert_eq!(
        last.applied_at,
        Some(at(5)),
        "an unset field keeps its value"
    );
    assert_eq!(last.existing_flows.as_deref(), Some("n/a"));
    assert_eq!(last.deadline, Some(at(607)));
    assert_eq!(last.created_at, at(1));
}

async fn a_stale_guard_or_a_missing_block_is_a_conflict(
    store: &(impl UnitOfWork + ContainmentRepo),
) {
    let row = block(HostId::new(), 1);
    store.commit(create(&row)).await.unwrap();
    store
        .commit(phase(&row, PhaseUpdate::to(BlockPhase::Applied, at(2))))
        .await
        .unwrap();

    // The same guard again: the version moved on.
    let stale = store
        .commit(phase(&row, PhaseUpdate::to(BlockPhase::Failed, at(3))))
        .await;
    match stale {
        Err(CommitError::Conflict {
            expected, found, ..
        }) => {
            assert_eq!(expected, Expectation::Version(1));
            assert_eq!(found, Observed::Version(2));
        }
        other => panic!("{other:?}"),
    }
    // Right version, wrong phase.
    let mut wrong = reread(store, &row).await;
    wrong.phase = BlockPhase::Queued;
    assert!(matches!(
        store
            .commit(phase(&wrong, PhaseUpdate::to(BlockPhase::Failed, at(3))))
            .await,
        Err(CommitError::Conflict { .. })
    ));
    assert_eq!(reread(store, &row).await.phase, BlockPhase::Applied);

    // A block nobody created.
    let ghost = block(HostId::new(), 1);
    assert!(matches!(
        store
            .commit(phase(&ghost, PhaseUpdate::to(BlockPhase::Applied, at(3))))
            .await,
        Err(CommitError::Conflict {
            found: Observed::Missing,
            ..
        })
    ));
}

async fn a_second_create_is_a_conflict_and_executions_are_unique(
    store: &(impl UnitOfWork + ContainmentRepo),
) {
    let first = block(HostId::new(), 1);
    store.commit(create(&first)).await.unwrap();
    assert!(matches!(
        store.commit(create(&first)).await,
        Err(CommitError::Conflict {
            expected: Expectation::New,
            ..
        })
    ));

    let shared = ExecutionId::from_step(StepId::new());
    let mut second = block(HostId::new(), 2);
    second.confirm_execution = Some(shared);
    store.commit(create(&second)).await.unwrap();
    let mut third = block(HostId::new(), 3);
    third.confirm_execution = Some(shared);
    assert!(matches!(
        store.commit(create(&third)).await,
        Err(CommitError::Constraint(_))
    ));
    assert!(store.get_block(third.block_id).await.unwrap().is_none());

    let mut confirming = PhaseUpdate::to(BlockPhase::Confirming, at(4));
    confirming.confirm_execution = Some(shared);
    assert!(matches!(
        store.commit(phase(&first, confirming)).await,
        Err(CommitError::Constraint(_))
    ));
    assert_eq!(reread(store, &first).await.phase, BlockPhase::Applying);
}

async fn listings_by_host_step_and_reconcile_phase(store: &(impl UnitOfWork + ContainmentRepo)) {
    let host = HostId::new();
    let other = HostId::new();
    let mut rows = Vec::new();
    for (created, phase) in [
        (1, BlockPhase::Applying),
        (2, BlockPhase::Confirmed),
        (3, BlockPhase::RollingBack),
        (4, BlockPhase::Expired),
        (5, BlockPhase::OutcomeUnknown),
    ] {
        let mut row = block(host, created);
        row.phase = phase;
        store.commit(create(&row)).await.unwrap();
        rows.push(row);
    }
    store.commit(create(&block(other, 6))).await.unwrap();

    let all = store.blocks_for_host(host, 10).await.unwrap();
    let created: Vec<_> = all.iter().map(|row| row.created_at).collect();
    assert_eq!(created, [at(5), at(4), at(3), at(2), at(1)], "newest first");
    assert_eq!(store.blocks_for_host(host, 2).await.unwrap().len(), 2);

    let pending = store.reconcilable_blocks(host).await.unwrap();
    let phases: Vec<_> = pending.iter().map(|row| row.phase).collect();
    assert_eq!(phases, [BlockPhase::Applying, BlockPhase::RollingBack]);

    let by_step = store
        .block_for_step(rows[1].operation_id, rows[1].step_id)
        .await
        .unwrap();
    assert_eq!(by_step.map(|row| row.block_id), Some(rows[1].block_id));
    assert!(
        store
            .block_for_step(OperationId::new(), StepId::new())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sqlite_a_block_round_trips_and_phase_moves_keep_unchanged_fields() {
    let fresh = Fresh::new().await;
    a_block_round_trips_and_phase_moves_keep_unchanged_fields(&fresh.store).await;
}

#[tokio::test]
async fn memory_a_block_round_trips_and_phase_moves_keep_unchanged_fields() {
    a_block_round_trips_and_phase_moves_keep_unchanged_fields(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn sqlite_a_stale_guard_or_a_missing_block_is_a_conflict() {
    let fresh = Fresh::new().await;
    a_stale_guard_or_a_missing_block_is_a_conflict(&fresh.store).await;
}

#[tokio::test]
async fn memory_a_stale_guard_or_a_missing_block_is_a_conflict() {
    a_stale_guard_or_a_missing_block_is_a_conflict(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn sqlite_a_second_create_is_a_conflict_and_executions_are_unique() {
    let fresh = Fresh::new().await;
    a_second_create_is_a_conflict_and_executions_are_unique(&fresh.store).await;
}

#[tokio::test]
async fn memory_a_second_create_is_a_conflict_and_executions_are_unique() {
    a_second_create_is_a_conflict_and_executions_are_unique(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn sqlite_listings_by_host_step_and_reconcile_phase() {
    let fresh = Fresh::new().await;
    listings_by_host_step_and_reconcile_phase(&fresh.store).await;
}

#[tokio::test]
async fn memory_listings_by_host_step_and_reconcile_phase() {
    listings_by_host_step_and_reconcile_phase(&InMemoryStore::new()).await;
}

#[tokio::test]
async fn a_phase_move_and_its_journal_row_commit_together_or_not_at_all() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let row = block(HostId::new(), 1);
    store.commit(create(&row)).await.unwrap();
    let draft = phase_journal(
        &row,
        BlockPhase::Applying,
        BlockPhase::Applied,
        None,
        Actor::system("containment").unwrap(),
        at(2),
    );
    assert_eq!(draft.kind.as_str(), "containment.phase");
    let ok = phase(&row, PhaseUpdate::to(BlockPhase::Applied, at(2))).journal(draft.clone());
    store.commit(ok).await.unwrap();

    // A stale guard rolls the journal row back with it.
    let stale = phase(&row, PhaseUpdate::to(BlockPhase::Failed, at(3))).journal(draft);
    assert!(store.commit(stale).await.is_err());
    let rows: (i64,) =
        sqlx::query_as("SELECT count(*) FROM journal WHERE kind = 'containment.phase'")
            .fetch_one(store.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(rows.0, 1);
}

#[tokio::test]
async fn purge_deletes_only_old_blocks_in_an_end_state() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let host = HostId::new();
    let day = 24 * 3600;
    let mut kept = Vec::new();
    let mut gone = Vec::new();
    for phase in BlockPhase::ALL {
        let mut old = block(host, 1);
        old.phase = phase;
        store.commit(create(&old)).await.unwrap();
        if phase.is_final() {
            gone.push(old.block_id);
        } else {
            kept.push(old.block_id);
        }
    }
    let mut recent = block(host, 300 * day);
    recent.phase = BlockPhase::Expired;
    store.commit(create(&recent)).await.unwrap();
    kept.push(recent.block_id);

    let now = at(301 * day);
    let deleted = store
        .purge_containment_blocks(now, SignedDuration::from_hours(24 * 180))
        .await
        .unwrap();
    assert_eq!(deleted, gone.len() as u64);
    for id in gone {
        assert!(store.get_block(id).await.unwrap().is_none());
    }
    for id in kept {
        assert!(store.get_block(id).await.unwrap().is_some());
    }
    assert!(!PURGED_TABLES.contains(&"containment_blocks"));
}
