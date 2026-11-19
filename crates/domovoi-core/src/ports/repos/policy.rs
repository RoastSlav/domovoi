//! Reading policy versions and shadow decisions. There is no write method: policy changes only
//! through the unit of work, as a `StateWrite::Policy`.

use async_trait::async_trait;
use jiff::Timestamp;

use super::{Page, Paged, StoreError};
use crate::id::{ExecutionId, PolicyVersionId};
use crate::policy_store::{
    AckRecord, PolicyVersionRecord, ShadowCandidate, ShadowFilter, ShadowRecord, ShadowState,
    ShadowSummary, StoredRule,
};

/// Read access to the policy store.
#[async_trait]
pub trait PolicyRepo: Send + Sync {
    /// The published version with the highest number, if any: the active version.
    async fn active(&self) -> Result<Option<PolicyVersionRecord>, StoreError>;

    /// The published version with this number.
    async fn version(&self, number: u32) -> Result<Option<PolicyVersionRecord>, StoreError>;

    /// The version, draft or published, with this id.
    async fn by_id(&self, id: PolicyVersionId) -> Result<Option<PolicyVersionRecord>, StoreError>;

    /// Published versions, newest first.
    async fn versions(&self, page: &Page) -> Result<Paged<PolicyVersionRecord>, StoreError>;

    /// The draft, if there is one.
    async fn draft(&self) -> Result<Option<PolicyVersionRecord>, StoreError>;

    /// The rules of one version in ordinal order.
    async fn rules(&self, id: PolicyVersionId) -> Result<Vec<StoredRule>, StoreError>;

    /// The acknowledgements stored with one version, ordered by finding key.
    async fn acknowledgements(&self, id: PolicyVersionId) -> Result<Vec<AckRecord>, StoreError>;

    /// Execution steps with no decision row, oldest first (creation time, then execution id),
    /// at most `limit`.
    async fn shadow_candidates(&self, limit: u16) -> Result<Vec<ShadowCandidate>, StoreError>;

    /// The decision row of one execution.
    async fn shadow(&self, execution: ExecutionId) -> Result<Option<ShadowRecord>, StoreError>;

    /// Decision rows, newest `evaluated_at` first; the cursor runs over
    /// `(evaluated_at, execution_id)`.
    async fn shadow_records(
        &self,
        filter: &ShadowFilter,
        page: &Page,
    ) -> Result<Paged<ShadowRecord>, StoreError>;

    /// Counts of the rows evaluated at or after `since`.
    async fn shadow_summary(&self, since: Timestamp) -> Result<ShadowSummary, StoreError>;

    /// The recorder's bookkeeping.
    async fn shadow_state(&self) -> Result<ShadowState, StoreError>;
}
