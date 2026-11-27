//! At start-up: security verdicts an older build stored.

use domovoi_core::Verdict;
use domovoi_core::ports::{IncidentFilter, IncidentRepo, Page, StoreError};
use domovoi_core::triage::is_security_check;

use crate::manager::ManagerHandle;

/// Withdraws every stored `EXPECTED` verdict on a security incident that no person gave.
/// Safe to run at every start. An incident that cannot be updated is logged and left for
/// the next start; the others still run.
pub async fn recap_security_verdicts(
    incidents: &dyn IncidentRepo,
    manager: &ManagerHandle,
) -> Result<(), StoreError> {
    let filter = IncidentFilter::default();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = incidents.list(&filter, &page).await?;
        for incident in got.items {
            if is_security_check(incident.check())
                && incident.verdict() == Some(Verdict::Expected)
                && let Err(err) = manager.recap_security_verdict(incident.id()).await
            {
                tracing::warn!(%err, incident = %incident.id(), "could not withdraw a stored verdict");
            }
        }
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(()),
        }
    }
}
