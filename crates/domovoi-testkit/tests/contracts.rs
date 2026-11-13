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
