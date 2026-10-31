//! Every repository contract suite from testkit, unchanged, against SQLite. Each suite gets
//! its own fresh database.

use crate::common;

macro_rules! contract_tests {
    ($($name:ident => $suite:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                let fresh = common::Fresh::new().await;
                domovoi_testkit::contracts::$suite(&fresh.store).await.unwrap();
            }
        )*
    };
}

contract_tests! {
    sqlite_host_repo_satisfies_contract => host_repo_contract,
    enrollment_token_is_single_use_in_sqlite => enrollment_token_is_single_use,
    expired_token_is_rejected_in_sqlite => expired_token_is_rejected,
    sqlite_signal_repo_satisfies_contract => signal_repo_contract,
    sqlite_incident_repo_satisfies_contract => incident_repo_contract,
    sqlite_action_repo_satisfies_contract => action_repo_contract,
    sqlite_audit_log_satisfies_contract => audit_log_contract,
    sqlite_note_repo_satisfies_contract => note_repo_contract,
    sqlite_baseline_repo_satisfies_contract => baseline_repo_contract,
    sqlite_ack_repo_satisfies_contract => ack_repo_contract,
    sqlite_advisory_repo_satisfies_contract => advisory_repo_contract,
    sqlite_fact_repo_satisfies_contract => fact_repo_contract,
    sqlite_triage_cache_repo_satisfies_contract => triage_cache_repo_contract,
    sqlite_usage_repo_satisfies_contract => usage_repo_contract,
    sqlite_settings_repo_satisfies_contract => settings_repo_contract,
    sqlite_user_repo_satisfies_contract => user_repo_contract,
    sqlite_session_repo_satisfies_contract => session_repo_contract,
}
