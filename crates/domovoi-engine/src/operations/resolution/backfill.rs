//! Start-up pass that queues a summary for each finished operation that has none.

use crate::operations::writer::OpShared;

/// How many operations one pass queues.
const BATCH: u16 = 500;

/// Queues one `draft_resolution` per finished operation without a summary and returns how
/// many it queued. Runs before the writer starts, so it commits directly; queueing is
/// idempotent: one already queued is left alone.
pub(in crate::operations) async fn queue_missing(shared: &OpShared) -> usize {
    let missing = match shared.deps.experience.undrafted(BATCH).await {
        Ok(missing) => missing,
        Err(err) => {
            tracing::warn!(%err, "could not list the operations without a summary");
            return 0;
        }
    };
    let mut queued = 0;
    for operation in missing {
        match crate::operations::writer::queue_resolution(shared, operation).await {
            Ok(()) => queued += 1,
            Err(err) => tracing::warn!(%err, %operation, "could not queue a summary"),
        }
    }
    queued
}
