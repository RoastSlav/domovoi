//! The knowledge sweeper: marks facts and relations stale once their freshness has run out.

use std::sync::Arc;

use domovoi_core::ports::Clock;
use domovoi_engine::knowledge::{KnowledgeDeps, KnowledgeService, UnavailableCoverage};
use domovoi_store::Store;
use jiff::SignedDuration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::runtime::every::every;

/// The knowledge service over the store. The helper attestation that makes management-path
/// coverage checkable arrives later; until then no path can be confirmed.
pub(crate) fn build(store: &Arc<Store>, clock: &Arc<dyn Clock>) -> Arc<KnowledgeService> {
    Arc::new(KnowledgeService::new(KnowledgeDeps {
        uow: store.clone(),
        read: store.clone(),
        coverage: Arc::new(UnavailableCoverage),
        clock: clock.clone(),
    }))
}

/// How often the sweep runs.
const PERIOD: SignedDuration = SignedDuration::from_secs(3600);

/// Starts the sweeper: one sweep at once, then one an hour, until `shutdown`. A failed sweep
/// is logged and the next run continues; it never stops the hub.
pub fn spawn_sweeper(
    service: Arc<KnowledgeService>,
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(every(clock, PERIOD, shutdown, move || {
        let service = service.clone();
        async move {
            match service.sweep().await {
                Ok(report) => tracing::debug!(
                    facts = report.marked_stale,
                    relations = report.relations_marked_stale,
                    "knowledge sweep finished"
                ),
                Err(err) => tracing::warn!(%err, "knowledge sweep failed"),
            }
        }
    }))
}
