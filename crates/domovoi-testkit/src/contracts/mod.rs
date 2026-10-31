//! Contract suites for the repository ports.
//!
//! Each suite takes an empty repository, exercises the behaviour documented on its port and
//! fails an `assert!` on any violation. Setup or backend failures come back as
//! [`ContractError`]. `domovoi-store` runs the same suites against SQLite.

mod accounts;
mod actions;
mod hosts;
mod incidents;
mod knowledge;
mod triage;

pub use accounts::{session_repo_contract, settings_repo_contract, user_repo_contract};
pub use actions::{action_repo_contract, audit_log_contract};
pub use hosts::{enrollment_token_is_single_use, expired_token_is_rejected, host_repo_contract};
pub use incidents::{incident_repo_contract, signal_repo_contract};
pub use knowledge::{
    ack_repo_contract, advisory_repo_contract, baseline_repo_contract, fact_repo_contract,
    note_repo_contract,
};
pub use triage::{triage_cache_repo_contract, usage_repo_contract};

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
}

/// What every contract suite returns.
pub type ContractResult = Result<(), ContractError>;
