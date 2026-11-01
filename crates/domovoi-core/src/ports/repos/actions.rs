//! Action requests.

use async_trait::async_trait;

use super::{Page, Paged, StoreError};
use crate::action::ActionRequest;
use crate::id::{ActionId, HostId, IncidentId};

/// Which actions a listing returns. The default returns all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionFilter {
    /// Only actions for this incident.
    pub incident: Option<IncidentId>,
    /// Only actions on this host.
    pub host: Option<HostId>,
    /// Only actions with this status name (see
    /// [`ActionStatus::NAMES`](crate::ActionStatus::NAMES)).
    pub status: Option<String>,
}

/// Stored action requests. Listings order by `created_at`, newest first.
#[async_trait]
pub trait ActionRepo: Send + Sync {
    /// Stores a new request. [`StoreError::Conflict`] when the id or the chat token hash is
    /// already stored.
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError>;

    /// Replaces the stored request with the same id.
    async fn update(&self, action: &ActionRequest) -> Result<(), StoreError>;

    /// The request with `id`.
    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError>;

    /// One page of requests matching `filter`.
    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError>;

    /// The request whose chat approval token hashes to `hash`, in any status. The caller
    /// checks status and expiry.
    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError>;
}
