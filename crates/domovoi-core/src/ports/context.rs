//! A read-only provider of context items.

use async_trait::async_trait;

use crate::context::{ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId};

/// A read-only provider of context items.
///
/// Every implementation must be read-only (no write, effect, event or approval), bounded by the
/// query's limit and byte bounds, redacted, scoped to the query's scope, and must never answer
/// "nothing" for a read that failed: a failure is an item with `ItemStatus::Unknown` or a
/// gap. Nothing runs after the query's deadline. An item carries evidence only; past
/// operations are `Origin::Historical`. The same fact yields the same item id every time.
#[async_trait]
pub trait ContextSource: Send + Sync {
    /// The source's name.
    fn id(&self) -> ContextSourceId;
    /// The layer every item of this source belongs to.
    fn layer(&self) -> ContextLayer;
    /// Whether [`fetch`](Self::fetch) answers this selector. A caller must not send one that is
    /// not supported.
    fn supports(&self, selector: &ContextSelector) -> bool;
    /// Answers the query. Never panics, never writes, never errors: a failure becomes a gap.
    async fn fetch(&self, query: &ContextQuery) -> ContextBatch;
}
