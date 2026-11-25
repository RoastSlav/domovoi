//! The knowledge sweeper: marks facts and relations stale once their freshness has run out.

use std::sync::Arc;

use domovoi_core::ports::{Clock, HelperTrustRead};
use domovoi_engine::knowledge::{AttestedCoverage, KnowledgeDeps, KnowledgeService};
use domovoi_store::Store;
use jiff::SignedDuration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::runtime::every::every;

/// The knowledge service over the store. A management path can be confirmed only when the
/// helper's verified attestation, read through `trust`, shows it covered.
pub(crate) fn build(
    store: &Arc<Store>,
    clock: &Arc<dyn Clock>,
    trust: Arc<dyn HelperTrustRead>,
) -> Arc<KnowledgeService> {
    Arc::new(KnowledgeService::new(KnowledgeDeps {
        uow: store.clone(),
        read: store.clone(),
        coverage: Arc::new(AttestedCoverage::new(trust, clock.clone())),
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
