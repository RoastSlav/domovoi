//! Errors the agent runtime reports.

use std::path::PathBuf;

use domovoi_proto::ProtocolVersion;

/// Why a file under the state directory could not be read or written.
#[derive(Debug, thiserror::Error)]
#[error("{path}: {reason}")]
pub struct StateError {
    /// The file.
    pub path: PathBuf,
    /// What went wrong.
    pub reason: String,
}

impl StateError {
    /// A state error for `path`.
    pub fn new(path: impl Into<PathBuf>, reason: impl std::fmt::Display) -> Self {
        Self {
            path: path.into(),
            reason: reason.to_string(),
        }
    }
}

/// Why connecting to the hub or keeping the link up failed.
#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// The hub URL cannot be dialled.
    #[error("hub url {0} cannot be used: {1}")]
    BadUrl(String, String),
    /// TCP, TLS or WebSocket setup failed; the link retries.
    #[error("could not connect to the hub: {0}")]
    Connect(String),
    /// The hub presented a certificate other than the pinned one.
    #[error("the hub presented a certificate whose fingerprint is not the pinned one")]
    CertificateMismatch,
    /// Connecting took longer than the connect timeout.
    #[error("connecting to the hub timed out")]
    Timeout,
    /// The connection broke or the hub stopped answering; the link retries.
    #[error("the hub connection was lost: {0}")]
    Lost(String),
    /// The hub broke the protocol; the link retries.
    #[error("the hub sent an unexpected message: {0}")]
    Protocol(String),
    /// The hub has revoked this agent or does not know it. The link stops.
    #[error("the hub rejected this agent ({0}); enroll it again")]
    Revoked(&'static str),
    /// The hub speaks an incompatible protocol version. The link stops.
    #[error(
        "the hub speaks protocol {}.{} and this agent speaks {}.{}; upgrade the older one",
        hub.major, hub.minor, agent.major, agent.minor
    )]
    Incompatible {
        /// The hub's protocol version.
        hub: ProtocolVersion,
        /// This agent's protocol version.
        agent: ProtocolVersion,
    },
}

impl LinkError {
    /// Whether retrying cannot help, so the link stops.
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Revoked(_) | Self::Incompatible { .. })
    }
}

/// Why enrollment failed.
#[derive(Debug, thiserror::Error)]
pub enum EnrollError {
    /// Connecting to the hub failed.
    #[error(transparent)]
    Link(#[from] LinkError),
    /// The hub refused the enrollment token.
    #[error("the hub refused the enrollment token; it may be used or expired")]
    BadToken,
    /// The hub refused the enrollment for another reason.
    #[error("the hub refused the enrollment: {0}")]
    Rejected(String),
    /// The fingerprint the hub reported differs from the certificate it presented.
    #[error("the hub reported a certificate fingerprint other than the one it presented")]
    FingerprintMismatch,
    /// The identity could not be saved.
    #[error(transparent)]
    State(#[from] StateError),
}

/// Why the agent runtime could not start or stopped.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// There is no identity yet; run enrollment first.
    #[error("this agent is not enrolled; no identity at {}", .0.display())]
    NotEnrolled(PathBuf),
    /// A state file could not be read or written.
    #[error(transparent)]
    State(#[from] StateError),
    /// The check configuration is invalid.
    #[error("invalid check configuration: {0}")]
    Checks(String),
    /// The link stopped for good.
    #[error(transparent)]
    Link(#[from] LinkError),
}
