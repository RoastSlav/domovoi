//! Reading resolution summaries and searching past operations.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::experience::{
    ExperienceHit, ExperienceQuery, ResolutionBrief, ResolutionSummary, SimilarHit, SimilarQuery,
    SummaryRevision,
};
use crate::id::OperationId;

use super::repos::StoreError;

/// Read access to the experience that finished operations leave behind.
///
/// There is no write method: every write is an `OperationWrite::SummaryField` inside a
/// transaction built by the operation writer.
#[async_trait]
pub trait ExperienceRead: Send + Sync {
    /// The current summary of `operation`; `None` when no revision exists yet.
    async fn summary(
        &self,
        operation: OperationId,
    ) -> Result<Option<ResolutionSummary>, StoreError>;

    /// Every revision of every field, ordered by (field in `SummaryField::ALL` order, revision).
    async fn revisions(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError>;

    /// The newest revision of each field that has one, in `SummaryField::ALL` order.
    async fn current(&self, operation: OperationId) -> Result<Vec<SummaryRevision>, StoreError>;

    /// Outcome and verified flag of each operation that has a summary; other ids are absent.
    async fn briefs(
        &self,
        operations: &[OperationId],
    ) -> Result<BTreeMap<OperationId, ResolutionBrief>, StoreError>;

    /// Full-text search over finished operations. A host scope returns only operations whose
    /// host set is exactly that host. No words lists the newest operations of the scope.
    /// Hits matching every word come before hits matching any word.
    async fn search(&self, query: &ExperienceQuery) -> Result<Vec<ExperienceHit>, StoreError>;

    /// Past operations of `query.host` that resemble the query, best first, each scoring at
    /// least `SIMILAR_MIN_SCORE`.
    async fn similar(&self, query: &SimilarQuery) -> Result<Vec<SimilarHit>, StoreError>;

    /// Terminal operations that have no revision at all, oldest `ended_at` first.
    async fn undrafted(&self, limit: u16) -> Result<Vec<OperationId>, StoreError>;
}
