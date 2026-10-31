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
