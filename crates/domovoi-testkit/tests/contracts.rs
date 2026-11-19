//! Every repository contract suite, run against the in-memory store.

use domovoi_testkit::InMemoryStore;
use domovoi_testkit::contracts::*;

macro_rules! contract_tests {
    ($($name:ident => $suite:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                $suite(&InMemoryStore::new()).await.unwrap();
            }
        )*
    };
}

contract_tests! {
    in_memory_host_repo_satisfies_contract => host_repo_contract,
    enrollment_token_is_single_use_in_memory => enrollment_token_is_single_use,
    expired_token_is_rejected_in_memory => expired_token_is_rejected,
    in_memory_signal_repo_satisfies_contract => signal_repo_contract,
    in_memory_incident_repo_satisfies_contract => incident_repo_contract,
    in_memory_action_repo_satisfies_contract => action_repo_contract,
    in_memory_audit_log_satisfies_contract => audit_log_contract,
    in_memory_note_repo_satisfies_contract => note_repo_contract,
    in_memory_baseline_repo_satisfies_contract => baseline_repo_contract,
    in_memory_ack_repo_satisfies_contract => ack_repo_contract,
    in_memory_advisory_repo_satisfies_contract => advisory_repo_contract,
    in_memory_fact_repo_satisfies_contract => fact_repo_contract,
    in_memory_triage_cache_repo_satisfies_contract => triage_cache_repo_contract,
    in_memory_usage_repo_satisfies_contract => usage_repo_contract,
    in_memory_admission_repo_satisfies_contract => admission_repo_contract,
    in_memory_settings_repo_satisfies_contract => settings_repo_contract,
    in_memory_user_repo_satisfies_contract => user_repo_contract,
    in_memory_session_repo_satisfies_contract => session_repo_contract,
    in_memory_notify_state_repo_satisfies_contract => notify_state_repo_contract,
    in_memory_commit_applies_state_journal_audit_and_effects_together => commit_applies_state_journal_audit_and_effects_together,
    in_memory_empty_transaction_commits_nothing => empty_transaction_commits_nothing,
    in_memory_stale_incident_version_conflicts_and_changes_nothing => stale_incident_version_conflicts_and_changes_nothing,
    in_memory_new_incident_over_existing_id_conflicts => new_incident_over_existing_id_conflicts,
    in_memory_second_active_incident_for_a_fingerprint_is_refused_and_rolls_back => second_active_incident_for_a_fingerprint_is_refused_and_rolls_back,
    in_memory_later_failure_rolls_back_earlier_writes_journal_audit_and_effects => later_failure_rolls_back_earlier_writes_journal_audit_and_effects,
    in_memory_repository_writes_advance_versions_like_commits => repository_writes_advance_versions_like_commits,
    in_memory_action_guard_refuses_a_changed_status => action_guard_refuses_a_changed_status,
    in_memory_chat_token_hash_must_be_unique_across_actions => chat_token_hash_must_be_unique_across_actions,
    in_memory_incident_event_is_mirrored_into_the_journal_with_its_original_time => incident_event_is_mirrored_into_the_journal_with_its_original_time,
    in_memory_journal_sequence_follows_commit_order => journal_sequence_follows_commit_order,
    in_memory_discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals => discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals,
    in_memory_audit_draft_is_appended_with_the_next_sequence_number => audit_draft_is_appended_with_the_next_sequence_number,
    in_memory_duplicate_dedup_key_is_refused_while_pending => duplicate_dedup_key_is_refused_while_pending,
    in_memory_effect_completion_needs_the_current_claim_token => effect_completion_needs_the_current_claim_token,
    in_memory_completing_an_unclaimed_effect_without_a_token_marks_it_done => completing_an_unclaimed_effect_without_a_token_marks_it_done,
    in_memory_zero_max_attempts_is_refused => zero_max_attempts_is_refused,
    in_memory_effect_id_must_be_unique => effect_id_must_be_unique,
    in_memory_effects_are_listed_in_insertion_order => effects_are_listed_in_insertion_order,
    in_memory_failed_effect_completion_rolls_back_every_stage => failed_effect_completion_rolls_back_every_stage,
    in_memory_claim_prioritizes_pending_effects_before_expired_claims => claim_prioritizes_pending_effects_before_expired_claims,
    in_memory_claim_sets_token_generation_lease_and_attempt => claim_sets_token_generation_lease_and_attempt,
    in_memory_claim_skips_effects_not_yet_due_and_other_kinds => claim_skips_effects_not_yet_due_and_other_kinds,
    in_memory_claimed_effect_reclaimed_after_lease_expiry => claimed_effect_reclaimed_after_lease_expiry,
    in_memory_stale_lease_holder_cannot_complete_reclaimed_effect => stale_lease_holder_cannot_complete_reclaimed_effect,
    in_memory_stale_holder_cannot_reschedule_or_dead_letter => stale_holder_cannot_reschedule_or_dead_letter,
    in_memory_reschedule_returns_effect_to_pending_with_backoff_time => reschedule_returns_effect_to_pending_with_backoff_time,
    in_memory_attempt_limit_dead_letters_at_claim => attempt_limit_dead_letters_at_claim,
    in_memory_expired_lease_beyond_attempt_limit_dies_instead_of_reclaim => expired_lease_beyond_attempt_limit_dies_instead_of_reclaim,
    in_memory_dispatch_effects_are_never_reclaimed_after_lease_expiry => dispatch_effects_are_never_reclaimed_after_lease_expiry,
    in_memory_restart_reverts_claimed_effects_except_dispatch => restart_reverts_claimed_effects_except_dispatch,
    in_memory_dedup_key_blocks_a_second_pending_effect => dedup_key_blocks_a_second_pending_effect,
    in_memory_claim_filters_by_aggregate_kind => claim_filters_by_aggregate_kind,
    in_memory_an_empty_aggregate_scope_claims_nothing => an_empty_aggregate_scope_claims_nothing,
    in_memory_revert_claimed_at_start_is_scoped_to_aggregates => revert_claimed_at_start_is_scoped_to_aggregates,
    in_memory_cancel_effects_cancels_only_pending_effects_of_the_aggregate => cancel_effects_cancels_only_pending_effects_of_the_aggregate,
    in_memory_operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together => operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together,
    in_memory_operation_new_over_existing_id_conflicts => operation_new_over_existing_id_conflicts,
    in_memory_operation_stale_version_conflicts_and_changes_nothing => operation_stale_version_conflicts_and_changes_nothing,
    in_memory_step_progress_is_guarded_by_the_stored_outcome => step_progress_is_guarded_by_the_stored_outcome,
    in_memory_step_ordinals_are_dense_and_unique_within_an_operation => step_ordinals_are_dense_and_unique_within_an_operation,
    in_memory_edge_to_a_later_or_equal_ordinal_is_refused => edge_to_a_later_or_equal_ordinal_is_refused,
    in_memory_edge_across_operations_is_refused => edge_across_operations_is_refused,
    in_memory_edge_to_a_missing_step_is_refused => edge_to_a_missing_step_is_refused,
    in_memory_execution_row_needs_an_execution_step_and_is_one_to_one => execution_row_needs_an_execution_step_and_is_one_to_one,
    in_memory_execution_version_guard_refuses_a_stale_write => execution_version_guard_refuses_a_stale_write,
    in_memory_execution_id_equal_to_an_action_id_is_refused => execution_id_equal_to_an_action_id_is_refused,
    in_memory_execution_round_trips_a_human_approval_exactly => execution_round_trips_a_human_approval_exactly,
    in_memory_snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions => snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions,
    in_memory_dispatched_on_lists_only_running_executions_of_the_host => dispatched_on_lists_only_running_executions_of_the_host,
    in_memory_dispatched_since_counts_by_host_contract_and_time => dispatched_since_counts_by_host_contract_and_time,
    in_memory_with_status_returns_oldest_first => with_status_returns_oldest_first,
    in_memory_incident_link_is_idempotent => incident_link_is_idempotent,
    in_memory_failed_operation_commit_rolls_back_every_stage => failed_operation_commit_rolls_back_every_stage,
    in_memory_operation_listing_filters_and_pages_newest_first => operation_listing_filters_and_pages_newest_first,
}

#[tokio::test]
async fn failed_commit_injected_by_fail_next_changes_nothing() {
    use domovoi_core::ports::{
        CommitError, Expected, StateWrite, StoreError, Transaction, UnitOfWork, VersionedRead,
    };
    use domovoi_testkit::build::{host, open_incident};

    let store = InMemoryStore::new();
    store.fail_next(StoreError::Backend("disk full".to_owned()));
    let host = host("nas").unwrap();
    let incident = open_incident(host.id, "disk.smart", None).unwrap();
    let failed = store
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: incident.clone(),
            expected: Expected::New,
        }))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::Backend(ref message)) if message.contains("disk full")
    ));
    assert_eq!(
        VersionedRead::incident(&store, incident.id())
            .await
            .unwrap(),
        None
    );
    store
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: incident.clone(),
            expected: Expected::New,
        }))
        .await
        .unwrap();
    assert_eq!(
        VersionedRead::incident(&store, incident.id())
            .await
            .unwrap()
            .map(|v| v.version),
        Some(1)
    );
}

#[tokio::test]
async fn injected_failures_surface_once() {
    use domovoi_core::ports::{SettingsRepo, StoreError};

    let store = InMemoryStore::new();
    store.fail_next(StoreError::Backend("disk full".to_owned()));
    assert_eq!(
        SettingsRepo::get(&store, "k").await,
        Err(StoreError::Backend("disk full".to_owned()))
    );
    assert_eq!(SettingsRepo::get(&store, "k").await, Ok(None));
}
