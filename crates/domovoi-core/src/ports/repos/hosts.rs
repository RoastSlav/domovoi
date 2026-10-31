//! Hosts, agents and enrollment tokens.

use async_trait::async_trait;
use jiff::Timestamp;

use super::StoreError;
use crate::host::{AgentIdentity, Host, HostName};
use crate::id::{AgentId, HostId};

/// A one-time token that lets an agent enroll for a host. Only its hash is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentToken {
    /// The host the agent will run on.
    pub host: HostId,
    /// SHA-256 of the token.
    pub token_hash: [u8; 32],
    /// The token is valid while `now < expires_at`.
    pub expires_at: Timestamp,
}

/// Hosts, their agents and enrollment tokens.
#[async_trait]
pub trait HostRepo: Send + Sync {
    /// Inserts a host or replaces the one with the same id. [`StoreError::Conflict`] when
    /// another host already has the name.
    async fn upsert_host(&self, host: &Host) -> Result<(), StoreError>;

    /// The host with `id`.
    async fn get_host(&self, id: HostId) -> Result<Option<Host>, StoreError>;

    /// The host named `name`.
    async fn get_host_by_name(&self, name: &HostName) -> Result<Option<Host>, StoreError>;

    /// Every host, sorted by name.
    async fn list_hosts(&self) -> Result<Vec<Host>, StoreError>;

    /// Records an enrolled agent, or replaces the record with the same agent id (for a new
    /// version after a handshake).
    ///
    /// [`StoreError::NotFound`] when the host does not exist. [`StoreError::Conflict`] when
    /// the host already has a different agent that is not revoked, or when `agent` was
    /// revoked: a revoked agent never comes back.
    async fn put_agent(&self, agent: &AgentIdentity) -> Result<(), StoreError>;

    /// The agent with `id`, revoked or not.
    async fn get_agent(&self, id: AgentId) -> Result<Option<AgentIdentity>, StoreError>;

    /// The agent enrolled for `host` that is not revoked, if any. A host has at most one.
    async fn live_agent(&self, host: HostId) -> Result<Option<AgentIdentity>, StoreError>;

    /// Marks an agent revoked at `at`. Revoking twice keeps the first time.
    async fn revoke_agent(&self, id: AgentId, at: Timestamp) -> Result<(), StoreError>;

    /// Stores a new enrollment token. [`StoreError::NotFound`] when the host does not exist,
    /// [`StoreError::Conflict`] when the hash is already stored.
    async fn create_enrollment_token(&self, token: &EnrollmentToken) -> Result<(), StoreError>;

    /// Uses up a token and returns its host. Single use: a second call with the same hash
    /// fails. Unknown, used and expired tokens all return [`StoreError::NotFound`], so a
    /// caller cannot tell them apart.
    async fn consume_enrollment_token(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<HostId, StoreError>;
}
