//! Why the hub could not be built, served or stopped.

use domovoi_llm::LlmBuildError;

use super::lock::LockError;
use crate::agent_server::AgentServerError;
use domovoi_store::StoreOpenError;

/// Why the hub could not be built, served or stopped.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// Opening or migrating SQLite failed.
    #[error(transparent)]
    StoreOpen(#[from] StoreOpenError),
    /// Persisted runtime state could not be read or written.
    #[error(transparent)]
    Store(#[from] domovoi_core::ports::StoreError),
    /// The capability registry could not be built.
    #[error(transparent)]
    Registry(#[from] crate::capability::RegistryError),
    /// Agent-server settings or its TLS identity failed.
    #[error(transparent)]
    Agent(#[from] AgentServerError),
    /// A language model adapter could not be built.
    #[error(transparent)]
    Llm(#[from] LlmBuildError),
    /// A chat adapter could not be built.
    #[error("could not build a chat adapter: {0}")]
    Chat(String),
    /// A URL derived from configuration was invalid.
    #[error("invalid hub URL: {0}")]
    Url(String),
    /// A configured listener could not be opened.
    #[error("could not bind {name}: {source}")]
    Bind {
        /// Which listener failed.
        name: &'static str,
        /// The operating-system error.
        source: std::io::Error,
    },
    /// A server task stopped unexpectedly.
    #[error("{0}")]
    Serve(String),
    /// Another hub process holds this data directory.
    #[error(transparent)]
    Locked(#[from] LockError),
    /// A key in `[packages]` cannot be used.
    #[error(transparent)]
    PackageKeys(#[from] crate::packages::keys::PackageKeysError),
    /// The package store could not be opened.
    #[error("{0}")]
    Packages(String),
    /// The settings could not be loaded.
    #[error(transparent)]
    Settings(#[from] crate::settings::LoadError),
    /// The runtime exceeded its shutdown budget.
    #[error("hub shutdown did not complete within 15 seconds")]
    ShutdownTimeout,
}
