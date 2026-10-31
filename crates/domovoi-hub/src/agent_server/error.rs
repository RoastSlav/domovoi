//! Errors from running the agent server.

use domovoi_core::ports::StoreError;

/// Why an agent server operation failed.
#[derive(Debug, thiserror::Error)]
pub enum AgentServerError {
    /// The TLS certificate or key could not be created, read or used.
    #[error("TLS identity in {path}: {reason}")]
    Tls {
        /// The directory holding the certificate and key.
        path: String,
        /// What went wrong.
        reason: String,
    },
    /// The settings cannot be used, for example a public URL that is not `wss://`.
    #[error("agent server settings: {0}")]
    Settings(String),
    /// The store failed.
    #[error("store: {0}")]
    Store(#[from] StoreError),
    /// The listener failed.
    #[error("agent listener: {0}")]
    Listen(String),
}
