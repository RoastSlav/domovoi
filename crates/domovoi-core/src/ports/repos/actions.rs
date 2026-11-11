//! Action requests.

use std::collections::BTreeMap;

use async_trait::async_trait;
use jiff::Timestamp;

use super::{Page, Paged, StoreError};
use crate::action::ActionRequest;
use crate::id::{ActionId, HostId, IncidentId};
use crate::skill::SkillName;

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
    /// `Some(true)`: only actions a person or time has decided (anything but `proposed`);
    /// `Some(false)`: only those still waiting.
    pub decided: Option<bool>,
    /// Only actions that last changed at or after this instant.
    pub updated_since: Option<Timestamp>,
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

    /// When each skill last ran on `host`: the latest change of any request for it that was
    /// sent to the agent (`dispatched`, `succeeded`, `failed`, `verified` or
    /// `verify_failed`). Requests that never reached the agent do not count, and a skill
    /// that never ran is absent.
    async fn last_run_by_skill(
        &self,
        host: HostId,
    ) -> Result<BTreeMap<SkillName, Timestamp>, StoreError>;
}

/// The status names of requests that reached the agent; see
/// [`ActionRepo::last_run_by_skill`].
pub const RAN_STATUSES: [&str; 5] = [
    "dispatched",
    "succeeded",
    "failed",
    "verified",
    "verify_failed",
];
