//! Contract suites for the repository ports.
//!
//! Each suite takes an empty repository, exercises the behaviour documented on its port and
//! fails an `assert!` on any violation. Setup or backend failures come back as
//! [`ContractError`]. `domovoi-store` runs the same suites against SQLite.

mod accounts;
mod admission;
mod audit;
pub mod context;
mod conversations;
mod experience;
mod experience_search;
mod hosts;
mod incident_groups;
mod incidents;
mod knowledge;
mod knowledge_store;
mod knowledge_store_more;
mod notify;
mod operations;
mod policy;
mod policy_shadow;
mod triage;
pub mod uow;

pub use accounts::{session_repo_contract, settings_repo_contract, user_repo_contract};
pub use admission::admission_repo_contract;
pub use audit::audit_log_contract;
pub use conversations::conversation_repo_contract;
pub use experience::*;
pub use experience_search::*;
pub use hosts::{enrollment_token_is_single_use, expired_token_is_rejected, host_repo_contract};
pub use incidents::{incident_repo_contract, signal_repo_contract};
pub use knowledge::{
    ack_repo_contract, advisory_repo_contract, baseline_repo_contract, metric_sample_repo_contract,
    note_repo_contract,
};
pub use knowledge_store::{
    KnowledgeSubject, confirmation_upserts_and_is_read_back,
    fact_insert_and_update_advance_versions_and_journal_together,
    live_value_is_unique_per_subject_and_predicate, retracted_fact_cannot_be_updated,
    retracted_fact_leaves_search_but_stays_listable_by_status,
    stale_fact_version_conflicts_and_changes_nothing,
};
pub use knowledge_store_more::{
    entity_in_use_sees_subjects_endpoints_and_depends_on_values,
    entity_names_are_unique_ignoring_case_and_versions_are_checked,
    facts_page_orders_newest_first_and_resumes_from_the_cursor, relation_live_edge_is_unique,
    search_matches_whole_words_in_live_facts_only,
};
pub use notify::notify_state_repo_contract;
pub use operations::*;
pub use policy::*;
pub use policy_shadow::*;
pub use triage::{triage_cache_repo_contract, usage_repo_contract};
pub use uow::*;

use domovoi_core::ports::StoreError;

use crate::build::BuildError;

/// A contract could not run to completion.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ContractError {
    /// The repository returned an error the contract did not expect.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Building test data failed.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// Preparing test data failed for another reason.
    #[error("setup failed: {0}")]
    Setup(String),
    /// The unit of work refused the transaction.
    #[error(transparent)]
    Commit(#[from] domovoi_core::ports::CommitError),
}

/// What every contract suite returns.
pub type ContractResult = Result<(), ContractError>;
