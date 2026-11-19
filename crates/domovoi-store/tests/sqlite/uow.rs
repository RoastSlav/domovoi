//! The unit-of-work contracts against SQLite, and what SQLite stores beyond them.

use async_trait::async_trait;
use domovoi_core::EffectId;
use domovoi_core::IncidentId;
use domovoi_core::ports::{
    ActionRepo, AggregateKind, AggregateRef, AuditLog, ClaimToken, CommitError, EffectDeath,
    EffectDraft, EffectKind, EffectQueue, EffectRead, EffectReschedule, Expected, IncidentRepo,
    IncidentTimelineEntry, JournalFeed, JournalRead, NoteRepo, OperationRepo, PolicyRepo,
    SignalRepo, StateWrite, Transaction, UnitOfWork, VersionedRead,
};
use domovoi_store::Store;
use domovoi_testkit::ContractError;
use domovoi_testkit::build::{T0, at, host, open_incident};
use domovoi_testkit::contracts::UowSubject;
use jiff::{SignedDuration, Timestamp};

use crate::common;

struct SqliteSubject(Store);

#[async_trait]
impl UowSubject for SqliteSubject {
    fn uow(&self) -> &dyn UnitOfWork {
        &self.0
    }

    fn versions(&self) -> &dyn VersionedRead {
        &self.0
    }

    fn feed(&self) -> &dyn JournalFeed {
        &self.0
    }

    fn journal(&self) -> &dyn JournalRead {
        &self.0
    }

    fn effects(&self) -> &dyn EffectRead {
        &self.0
    }

    fn queue(&self) -> &dyn EffectQueue {
        &self.0
    }

    fn incidents(&self) -> &dyn IncidentRepo {
        &self.0
    }

    fn signals(&self) -> &dyn SignalRepo {
        &self.0
    }

    fn actions(&self) -> &dyn ActionRepo {
        &self.0
    }

    fn notes(&self) -> &dyn NoteRepo {
        &self.0
    }

    fn audit(&self) -> &dyn AuditLog {
        &self.0
    }

    fn operations(&self) -> &dyn OperationRepo {
        &self.0
    }

    fn policy(&self) -> &dyn PolicyRepo {
        &self.0
    }

    async fn force_claim(
        &self,
        id: EffectId,
        token: ClaimToken,
        lease_until: Timestamp,
    ) -> Result<(), ContractError> {
        let done = sqlx::query(
            "UPDATE effects SET status = 'claimed', claim_token = ?, \
             claim_generation = claim_generation + 1, lease_until = ?, attempt = attempt + 1 \
             WHERE id = ?",
        )
        .bind(token.to_string())
        .bind(lease_until.as_millisecond())
        .bind(id.to_string())
        .execute(self.0.pool_for_tests())
        .await
        .map_err(|e| ContractError::Setup(e.to_string()))?;
        if done.rows_affected() == 0 {
            return Err(ContractError::Setup("the effect is missing".to_owned()));
        }
        Ok(())
    }
}

macro_rules! uow_contract_tests {
    ($($name:ident => $suite:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                let fresh = common::Fresh::new().await;
                let subject = SqliteSubject(fresh.store.clone());
                domovoi_testkit::contracts::$suite(&subject).await.unwrap();
            }
        )*
    };
}

uow_contract_tests! {
    sqlite_commit_applies_state_journal_audit_and_effects_together => commit_applies_state_journal_audit_and_effects_together,
    sqlite_empty_transaction_commits_nothing => empty_transaction_commits_nothing,
    sqlite_stale_incident_version_conflicts_and_changes_nothing => stale_incident_version_conflicts_and_changes_nothing,
    sqlite_new_incident_over_existing_id_conflicts => new_incident_over_existing_id_conflicts,
    sqlite_second_active_incident_for_a_fingerprint_is_refused_and_rolls_back => second_active_incident_for_a_fingerprint_is_refused_and_rolls_back,
    sqlite_later_failure_rolls_back_earlier_writes_journal_audit_and_effects => later_failure_rolls_back_earlier_writes_journal_audit_and_effects,
    sqlite_repository_writes_advance_versions_like_commits => repository_writes_advance_versions_like_commits,
    sqlite_action_guard_refuses_a_changed_status => action_guard_refuses_a_changed_status,
    sqlite_chat_token_hash_must_be_unique_across_actions => chat_token_hash_must_be_unique_across_actions,
    sqlite_incident_event_is_mirrored_into_the_journal_with_its_original_time => incident_event_is_mirrored_into_the_journal_with_its_original_time,
    sqlite_journal_sequence_follows_commit_order => journal_sequence_follows_commit_order,
    sqlite_floor_starts_at_zero_and_wait_after_follows_the_committed_head => floor_starts_at_zero_and_wait_after_follows_the_committed_head,
    sqlite_head_equals_the_highest_receipt_seq => head_equals_the_highest_receipt_seq,
    sqlite_discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals => discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals,
    sqlite_audit_draft_is_appended_with_the_next_sequence_number => audit_draft_is_appended_with_the_next_sequence_number,
    sqlite_duplicate_dedup_key_is_refused_while_pending => duplicate_dedup_key_is_refused_while_pending,
    sqlite_effect_completion_needs_the_current_claim_token => effect_completion_needs_the_current_claim_token,
    sqlite_completing_an_unclaimed_effect_without_a_token_marks_it_done => completing_an_unclaimed_effect_without_a_token_marks_it_done,
    sqlite_zero_max_attempts_is_refused => zero_max_attempts_is_refused,
    sqlite_effect_id_must_be_unique => effect_id_must_be_unique,
    sqlite_effects_are_listed_in_insertion_order => effects_are_listed_in_insertion_order,
    sqlite_failed_effect_completion_rolls_back_every_stage => failed_effect_completion_rolls_back_every_stage,
    sqlite_claim_prioritizes_pending_effects_before_expired_claims => claim_prioritizes_pending_effects_before_expired_claims,
    sqlite_claim_sets_token_generation_lease_and_attempt => claim_sets_token_generation_lease_and_attempt,
    sqlite_claim_skips_effects_not_yet_due_and_other_kinds => claim_skips_effects_not_yet_due_and_other_kinds,
    sqlite_claimed_effect_reclaimed_after_lease_expiry => claimed_effect_reclaimed_after_lease_expiry,
    sqlite_stale_lease_holder_cannot_complete_reclaimed_effect => stale_lease_holder_cannot_complete_reclaimed_effect,
    sqlite_stale_holder_cannot_reschedule_or_dead_letter => stale_holder_cannot_reschedule_or_dead_letter,
    sqlite_dispatch_effects_are_never_reclaimed_after_lease_expiry => dispatch_effects_are_never_reclaimed_after_lease_expiry,
    sqlite_restart_reverts_claimed_effects_except_dispatch => restart_reverts_claimed_effects_except_dispatch,
    sqlite_dedup_key_blocks_a_second_pending_effect => dedup_key_blocks_a_second_pending_effect,
    sqlite_claim_filters_by_aggregate_kind => claim_filters_by_aggregate_kind,
    sqlite_claimed_of_filters_by_kind_aggregate_and_status => claimed_of_filters_by_kind_aggregate_and_status,
    sqlite_an_empty_aggregate_scope_claims_nothing => an_empty_aggregate_scope_claims_nothing,
    sqlite_revert_claimed_at_start_is_scoped_to_aggregates => revert_claimed_at_start_is_scoped_to_aggregates,
    sqlite_cancel_effects_cancels_only_pending_effects_of_the_aggregate => cancel_effects_cancels_only_pending_effects_of_the_aggregate,
    sqlite_reschedule_returns_effect_to_pending_with_backoff_time => reschedule_returns_effect_to_pending_with_backoff_time,
    sqlite_attempt_limit_dead_letters_at_claim => attempt_limit_dead_letters_at_claim,
    sqlite_expired_lease_beyond_attempt_limit_dies_instead_of_reclaim => expired_lease_beyond_attempt_limit_dies_instead_of_reclaim,
    sqlite_operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together => operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together,
    sqlite_operation_new_over_existing_id_conflicts => operation_new_over_existing_id_conflicts,
    sqlite_operation_stale_version_conflicts_and_changes_nothing => operation_stale_version_conflicts_and_changes_nothing,
    sqlite_step_progress_is_guarded_by_the_stored_outcome => step_progress_is_guarded_by_the_stored_outcome,
    sqlite_step_ordinals_are_dense_and_unique_within_an_operation => step_ordinals_are_dense_and_unique_within_an_operation,
    sqlite_edge_to_a_later_or_equal_ordinal_is_refused => edge_to_a_later_or_equal_ordinal_is_refused,
    sqlite_edge_across_operations_is_refused => edge_across_operations_is_refused,
    sqlite_edge_to_a_missing_step_is_refused => edge_to_a_missing_step_is_refused,
    sqlite_execution_row_needs_an_execution_step_and_is_one_to_one => execution_row_needs_an_execution_step_and_is_one_to_one,
    sqlite_execution_version_guard_refuses_a_stale_write => execution_version_guard_refuses_a_stale_write,
    sqlite_execution_id_equal_to_an_action_id_is_refused => execution_id_equal_to_an_action_id_is_refused,
    sqlite_execution_round_trips_a_human_approval_exactly => execution_round_trips_a_human_approval_exactly,
    sqlite_snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions => snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions,
    sqlite_dispatched_on_lists_only_running_executions_of_the_host => dispatched_on_lists_only_running_executions_of_the_host,
    sqlite_dispatched_since_counts_by_host_contract_and_time => dispatched_since_counts_by_host_contract_and_time,
    sqlite_with_status_returns_oldest_first => with_status_returns_oldest_first,
    sqlite_incident_link_is_idempotent => incident_link_is_idempotent,
    sqlite_failed_operation_commit_rolls_back_every_stage => failed_operation_commit_rolls_back_every_stage,
    sqlite_operation_listing_filters_and_pages_newest_first => operation_listing_filters_and_pages_newest_first,
    sqlite_policy_draft_create_replace_discard_round_trip => draft_create_replace_discard_round_trip,
    sqlite_policy_second_draft_is_refused => second_draft_is_refused,
    sqlite_policy_stale_draft_revision_is_refused => stale_draft_revision_is_refused,
    sqlite_policy_publish_assigns_consecutive_numbers_and_freezes_rules => publish_assigns_consecutive_numbers_and_freezes_rules,
    sqlite_policy_publish_with_wrong_number_conflicts => publish_with_wrong_number_conflicts,
    sqlite_policy_acknowledgements_are_stored_with_the_version => acknowledgements_are_stored_with_the_version,
    sqlite_policy_shadow_candidates_are_unrecorded_executions_oldest_first => shadow_candidates_are_unrecorded_executions_oldest_first,
    sqlite_policy_record_shadow_is_idempotent_and_counts_inserted_rows => record_shadow_is_idempotent_and_counts_inserted_rows,
    sqlite_policy_shadow_records_filter_and_page_newest_first => shadow_records_filter_and_page_newest_first,
    sqlite_policy_shadow_summary_counts_by_status_and_outcome => shadow_summary_counts_by_status_and_outcome,
    sqlite_policy_unevaluable_rows_are_not_counted_as_differing => unevaluable_rows_are_not_counted_as_differing,
    sqlite_policy_empty_store_reads_are_empty => empty_store_reads_are_empty,
}

#[tokio::test]
async fn timeline_write_stores_legacy_event_seq() {
    let fresh = common::Fresh::new().await;
    let host = host("nas").unwrap();
    let open = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    IncidentRepo::insert(&fresh.store, &open).await.unwrap();
    let entry = IncidentTimelineEntry {
        incident: open.id(),
        at: at(500),
        kind: "status.changed".to_owned(),
        summary: "opened".to_owned(),
        detail: serde_json::Value::Null,
    };
    fresh
        .store
        .commit(Transaction::new().write(StateWrite::IncidentEvent(entry)))
        .await
        .unwrap();

    let legacy: (Option<i64>,) =
        sqlx::query_as("SELECT legacy_event_seq FROM journal WHERE kind = 'incident.timeline'")
            .fetch_one(fresh.store.pool_for_tests())
            .await
            .unwrap();
    let event: (i64,) = sqlx::query_as("SELECT seq FROM incident_events")
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(legacy.0, Some(event.0));
}

#[tokio::test]
async fn incident_body_carries_a_schema_version() {
    let fresh = common::Fresh::new().await;
    let host = host("nas").unwrap();
    let via_repo = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    IncidentRepo::insert(&fresh.store, &via_repo).await.unwrap();

    let via_commit = open_incident(host.id, "load.high", None).unwrap();
    fresh
        .store
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: via_commit.clone(),
            expected: Expected::New,
        }))
        .await
        .unwrap();

    for id in [via_repo.id(), via_commit.id()] {
        let body: (String,) = sqlx::query_as("SELECT body FROM incidents WHERE id = ?")
            .bind(id.to_string())
            .fetch_one(fresh.store.pool_for_tests())
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&body.0).unwrap();
        assert_eq!(value["schema_version"], 1);
    }

    // A body written before the key existed still loads.
    let body: (String,) = sqlx::query_as("SELECT body FROM incidents WHERE id = ?")
        .bind(via_repo.id().to_string())
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    let mut old: serde_json::Value = serde_json::from_str(&body.0).unwrap();
    old.as_object_mut().unwrap().remove("schema_version");
    sqlx::query("UPDATE incidents SET body = ? WHERE id = ?")
        .bind(old.to_string())
        .bind(via_repo.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert!(
        IncidentRepo::get(&fresh.store, via_repo.id())
            .await
            .unwrap()
            .is_some()
    );
}

/// A pending effect of `kind`, due at `not_before`, allowed `max_attempts` attempts.
fn queue_effect(kind: EffectKind, not_before: Timestamp, max_attempts: u32) -> EffectDraft {
    EffectDraft {
        id: EffectId::new(),
        kind,
        aggregate: AggregateRef::incident(IncidentId::new()),
        payload: serde_json::json!({}),
        not_before,
        max_attempts,
        dedup_key: None,
        created_at: T0,
    }
}

async fn effect_row(store: &Store, id: EffectId) -> domovoi_core::ports::EffectRecord {
    EffectRead::effect(store, id).await.unwrap().unwrap()
}

#[tokio::test]
async fn claim_due_claims_a_due_effect_under_a_fresh_token() {
    let fresh = common::Fresh::new().await;
    let due = queue_effect(EffectKind::Wake, at(10), 3);
    let early = queue_effect(EffectKind::Wake, at(100), 3);
    fresh
        .store
        .commit(Transaction::new().effect(due.clone()).effect(early.clone()))
        .await
        .unwrap();

    let batch = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(20),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();

    assert!(batch.dead.is_empty());
    assert_eq!(batch.claimed.len(), 1);
    let claimed = &batch.claimed[0];
    assert_eq!(claimed.record.id, due.id);
    assert_eq!(claimed.record.status, "claimed");
    assert_eq!(claimed.record.attempt, 1);
    assert_eq!(claimed.record.claim_generation, 1);
    assert_eq!(claimed.record.claim_token, Some(claimed.token));
    assert_eq!(claimed.record.runner_id.as_deref(), Some("runner-a"));
    assert_eq!(claimed.record.lease_until, Some(at(50)));
    assert_eq!(effect_row(&fresh.store, early.id).await.status, "pending");
}

#[tokio::test]
async fn claim_due_reclaims_an_expired_lease_with_the_next_generation() {
    let fresh = common::Fresh::new().await;
    let draft = queue_effect(EffectKind::Wake, at(10), 3);
    fresh
        .store
        .commit(Transaction::new().effect(draft.clone()))
        .await
        .unwrap();
    let first = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();
    let first_token = first.claimed[0].token;

    let second = fresh
        .store
        .claim_due(
            "runner-b",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(60),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();
    let claimed = &second.claimed[0];
    assert_eq!(claimed.record.id, draft.id);
    assert_eq!(claimed.record.attempt, 2);
    assert_eq!(claimed.record.claim_generation, 2);
    assert_eq!(claimed.record.runner_id.as_deref(), Some("runner-b"));
    assert_ne!(claimed.token, first_token);
    assert_eq!(claimed.record.claim_token, Some(claimed.token));

    let stale = fresh
        .store
        .commit(
            Transaction::new().complete(domovoi_core::ports::EffectCompletion {
                id: draft.id,
                claim_token: Some(first_token),
                at: at(70),
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(stale, CommitError::StaleEffect { effect: draft.id });
}

#[tokio::test]
async fn claim_due_dead_letters_a_pending_effect_past_its_attempt_limit() {
    let fresh = common::Fresh::new().await;
    let draft = queue_effect(EffectKind::Wake, at(10), 1);
    fresh
        .store
        .commit(Transaction::new().effect(draft.clone()))
        .await
        .unwrap();
    fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(
        fresh
            .store
            .revert_claimed_at_start(&AggregateKind::LEGACY)
            .await
            .unwrap(),
        1
    );

    let batch = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(20),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();

    assert!(batch.claimed.is_empty());
    assert_eq!(batch.dead.len(), 1);
    let dead = &batch.dead[0];
    assert_eq!(dead.id, draft.id);
    assert_eq!(dead.status, "dead");
    assert_eq!(dead.last_error.as_deref(), Some("attempt limit reached"));
    assert_eq!(dead.completed_at, Some(at(20)));
    assert_eq!(dead.claim_token, None);
    assert_eq!(dead.lease_until, None);
    assert_eq!(dead.runner_id, None);
}

#[tokio::test]
async fn claim_due_dead_letters_an_expired_claim_past_its_attempt_limit() {
    let fresh = common::Fresh::new().await;
    let draft = queue_effect(EffectKind::Wake, at(10), 1);
    fresh
        .store
        .commit(Transaction::new().effect(draft.clone()))
        .await
        .unwrap();
    fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();

    let batch = fresh
        .store
        .claim_due(
            "runner-b",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(60),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();

    assert!(batch.claimed.is_empty());
    assert_eq!(batch.dead.len(), 1);
    assert_eq!(
        batch.dead[0].last_error.as_deref(),
        Some("lease expired too many times")
    );
    assert_eq!(batch.dead[0].completed_at, Some(at(60)));
}

#[tokio::test]
async fn claim_due_never_reclaims_an_expired_dispatch_effect() {
    let fresh = common::Fresh::new().await;
    let draft = queue_effect(EffectKind::Dispatch, at(10), 5);
    fresh
        .store
        .commit(Transaction::new().effect(draft.clone()))
        .await
        .unwrap();
    let first = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Dispatch],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();
    let token = first.claimed[0].token;

    let batch = fresh
        .store
        .claim_due(
            "runner-b",
            &[EffectKind::Dispatch],
            &AggregateKind::LEGACY,
            at(60),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();

    assert!(batch.claimed.is_empty());
    assert!(batch.dead.is_empty());
    let row = effect_row(&fresh.store, draft.id).await;
    assert_eq!(row.status, "claimed");
    assert_eq!(row.claim_token, Some(token));
    assert_eq!(row.runner_id.as_deref(), Some("runner-a"));
}

#[tokio::test]
async fn claim_due_claims_in_due_order_up_to_the_limit() {
    let fresh = common::Fresh::new().await;
    let late = queue_effect(EffectKind::Wake, at(10), 3);
    let first_due = queue_effect(EffectKind::Wake, at(5), 3);
    let second_due = queue_effect(EffectKind::Wake, at(5), 3);
    fresh
        .store
        .commit(
            Transaction::new()
                .effect(late.clone())
                .effect(first_due.clone())
                .effect(second_due.clone()),
        )
        .await
        .unwrap();

    let batch = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(20),
            SignedDuration::from_secs(30),
            2,
        )
        .await
        .unwrap();

    let ids: Vec<EffectId> = batch.claimed.iter().map(|c| c.record.id).collect();
    assert_eq!(ids, vec![first_due.id, second_due.id]);
    assert_eq!(effect_row(&fresh.store, late.id).await.status, "pending");
}

#[tokio::test]
async fn claim_due_ignores_kinds_it_was_not_given_and_empty_requests() {
    let fresh = common::Fresh::new().await;
    let model = queue_effect(EffectKind::CallModel, at(10), 3);
    fresh
        .store
        .commit(Transaction::new().effect(model.clone()))
        .await
        .unwrap();

    let none = fresh
        .store
        .claim_due(
            "runner-a",
            &[],
            &AggregateKind::LEGACY,
            at(20),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();
    let zero = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::CallModel],
            &AggregateKind::LEGACY,
            at(20),
            SignedDuration::from_secs(30),
            0,
        )
        .await
        .unwrap();
    let other_kind = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(20),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap();

    assert!(none.claimed.is_empty() && none.dead.is_empty());
    assert!(zero.claimed.is_empty() && zero.dead.is_empty());
    assert!(other_kind.claimed.is_empty() && other_kind.dead.is_empty());
    assert_eq!(effect_row(&fresh.store, model.id).await.status, "pending");
}

#[tokio::test]
async fn revert_claimed_at_start_returns_work_to_pending_but_leaves_dispatch() {
    let fresh = common::Fresh::new().await;
    let wake = queue_effect(EffectKind::Wake, at(10), 3);
    let dispatch = queue_effect(EffectKind::Dispatch, at(10), 3);
    fresh
        .store
        .commit(
            Transaction::new()
                .effect(wake.clone())
                .effect(dispatch.clone()),
        )
        .await
        .unwrap();
    let wake_claim = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(600),
            10,
        )
        .await
        .unwrap();
    let dispatch_claim = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Dispatch],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(600),
            10,
        )
        .await
        .unwrap();

    assert_eq!(
        fresh
            .store
            .revert_claimed_at_start(&AggregateKind::LEGACY)
            .await
            .unwrap(),
        1
    );

    let reverted = effect_row(&fresh.store, wake.id).await;
    assert_eq!(reverted.status, "pending");
    assert_eq!(reverted.claim_token, None);
    assert_eq!(reverted.lease_until, None);
    assert_eq!(reverted.runner_id, None);
    assert_eq!(reverted.claim_generation, 2);
    assert_eq!(reverted.attempt, 1);
    let held = effect_row(&fresh.store, dispatch.id).await;
    assert_eq!(held.status, "claimed");
    assert_eq!(held.claim_token, Some(dispatch_claim.claimed[0].token));
    assert_ne!(wake_claim.claimed[0].token, dispatch_claim.claimed[0].token);
}

#[tokio::test]
async fn reschedule_returns_a_claimed_effect_to_pending_fenced_by_its_token() {
    let fresh = common::Fresh::new().await;
    let draft = queue_effect(EffectKind::Wake, at(10), 3);
    fresh
        .store
        .commit(Transaction::new().effect(draft.clone()))
        .await
        .unwrap();
    let token = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap()
        .claimed[0]
        .token;

    let stale = fresh
        .store
        .commit(Transaction::new().reschedule(EffectReschedule {
            id: draft.id,
            claim_token: ClaimToken::new(),
            not_before: at(120),
            error: "ignored".to_owned(),
            at: at(40),
        }))
        .await
        .unwrap_err();
    assert_eq!(stale, CommitError::StaleEffect { effect: draft.id });
    assert_eq!(effect_row(&fresh.store, draft.id).await.status, "claimed");

    fresh
        .store
        .commit(Transaction::new().reschedule(EffectReschedule {
            id: draft.id,
            claim_token: token,
            not_before: at(120),
            error: "agent offline".to_owned(),
            at: at(40),
        }))
        .await
        .unwrap();

    let row = effect_row(&fresh.store, draft.id).await;
    assert_eq!(row.status, "pending");
    assert_eq!(row.not_before, at(120));
    assert_eq!(row.last_error.as_deref(), Some("agent offline"));
    assert_eq!(row.claim_token, None);
    assert_eq!(row.lease_until, None);
    assert_eq!(row.runner_id, None);
    assert_eq!(row.claim_generation, 1);
    assert_eq!(row.attempt, 1);
}

#[tokio::test]
async fn dead_lettering_a_claimed_effect_is_fenced_by_its_token() {
    let fresh = common::Fresh::new().await;
    let draft = queue_effect(EffectKind::Wake, at(10), 3);
    fresh
        .store
        .commit(Transaction::new().effect(draft.clone()))
        .await
        .unwrap();
    let token = fresh
        .store
        .claim_due(
            "runner-a",
            &[EffectKind::Wake],
            &AggregateKind::LEGACY,
            at(10),
            SignedDuration::from_secs(30),
            10,
        )
        .await
        .unwrap()
        .claimed[0]
        .token;

    let stale = fresh
        .store
        .commit(Transaction::new().kill(EffectDeath {
            id: draft.id,
            claim_token: ClaimToken::new(),
            error: "ignored".to_owned(),
            at: at(40),
        }))
        .await
        .unwrap_err();
    assert_eq!(stale, CommitError::StaleEffect { effect: draft.id });

    fresh
        .store
        .commit(Transaction::new().kill(EffectDeath {
            id: draft.id,
            claim_token: token,
            error: "payload rejected".to_owned(),
            at: at(40),
        }))
        .await
        .unwrap();

    let row = effect_row(&fresh.store, draft.id).await;
    assert_eq!(row.status, "dead");
    assert_eq!(row.last_error.as_deref(), Some("payload rejected"));
    assert_eq!(row.completed_at, Some(at(40)));
    assert_eq!(row.claim_token, None);
    assert_eq!(row.lease_until, None);
    assert_eq!(row.runner_id, None);
    assert_eq!(row.claim_generation, 1);
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct AdvisoryRow {
    id: String,
    kind: String,
    host_id: Option<String>,
    title: String,
    resolved_at: Option<i64>,
}

#[tokio::test]
async fn advisories_migration_keeps_existing_rows() {
    let fresh = common::Fresh::new().await;
    const INSERT: &str = "INSERT INTO advisories \
        (id, kind, host_id, title, detail, created_at, resolved_at) VALUES (?, ?, ?, ?, ?, ?, ?)";
    sqlx::query(INSERT)
        .bind("old")
        .bind("secret_exposed")
        .bind("host-1")
        .bind("Exposed secret")
        .bind("detail")
        .bind(1_000_i64)
        .bind(Some(2_000_i64))
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    // Migration 0007 dropped the kind CHECK, so a new kind inserts without a table rebuild.
    sqlx::query(INSERT)
        .bind("new")
        .bind("effect_dead")
        .bind(Option::<String>::None)
        .bind("Effect gave up")
        .bind("detail")
        .bind(3_000_i64)
        .bind(Option::<i64>::None)
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();

    let rows: Vec<AdvisoryRow> =
        sqlx::query_as("SELECT id, kind, host_id, title, resolved_at FROM advisories ORDER BY id")
            .fetch_all(fresh.store.pool_for_tests())
            .await
            .unwrap();

    assert_eq!(
        rows,
        vec![
            AdvisoryRow {
                id: "new".to_owned(),
                kind: "effect_dead".to_owned(),
                host_id: None,
                title: "Effect gave up".to_owned(),
                resolved_at: None,
            },
            AdvisoryRow {
                id: "old".to_owned(),
                kind: "secret_exposed".to_owned(),
                host_id: Some("host-1".to_owned()),
                title: "Exposed secret".to_owned(),
                resolved_at: Some(2_000),
            },
        ]
    );
}
