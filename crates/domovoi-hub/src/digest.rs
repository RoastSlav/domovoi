//! Shared digest assembly for the API, chat commands and the scheduler.

use std::sync::Arc;

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    AdvisoryRepo, Clock, FactRepo, HostRepo, IncidentFilter, IncidentRepo, Page, StoreError,
};
use domovoi_engine::digest::{DigestInputs, build_digest};
use jiff::SignedDuration;

/// Days of facts before today that trends are computed from.
pub(crate) const HISTORY_DAYS: i64 = 14;

/// The repositories and configuration used to render a digest.
pub(crate) struct DigestDeps {
    pub(crate) config: Arc<HubConfig>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) hosts: Arc<dyn HostRepo>,
    pub(crate) facts: Arc<dyn FactRepo>,
    pub(crate) incidents: Arc<dyn IncidentRepo>,
    pub(crate) advisories: Arc<dyn AdvisoryRepo>,
}

/// A rendered digest and the local date it covers.
pub(crate) struct RenderedDigest {
    pub(crate) date: String,
    pub(crate) markdown: String,
}

/// Why a digest could not be assembled.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DigestError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not find the start of the local digest day: {0}")]
    DayStart(String),
}

pub(crate) async fn build(deps: &DigestDeps) -> Result<RenderedDigest, DigestError> {
    let tz = deps.config.hub.timezone.zone().clone();
    let now = deps.clock.now();
    let date = now.to_zoned(tz.clone()).date();
    let start = date
        .to_zoned(tz.clone())
        .map_err(|err| DigestError::DayStart(err.to_string()))?
        .timestamp();
    let history_start = start
        .checked_sub(SignedDuration::from_hours(24 * HISTORY_DAYS))
        .unwrap_or(start);
    let hosts = deps.hosts.list_hosts().await?;
    let mut facts_today = Vec::new();
    let mut facts_history = Vec::new();
    for host in &hosts {
        facts_today.extend(deps.facts.list(host.id, start, now).await?);
        facts_history.extend(deps.facts.list(host.id, history_start, start).await?);
    }
    let active = IncidentFilter {
        active: Some(true),
        ..IncidentFilter::default()
    };
    let open_incidents = deps
        .incidents
        .list(&active, &Page::first(Page::MAX_LIMIT))
        .await?
        .items;
    let advisories = deps.advisories.list_open().await?;
    let digest = build_digest(&DigestInputs {
        date,
        hosts: &hosts,
        facts_today: &facts_today,
        facts_history: &facts_history,
        open_incidents: &open_incidents,
        advisories: &advisories,
        tz: &tz,
        now,
    });
    Ok(RenderedDigest {
        date: date.to_string(),
        markdown: domovoi_engine::render::digest(&digest),
    })
}
