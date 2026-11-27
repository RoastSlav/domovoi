//! Past incidents of the same check on the same host, as history.

use async_trait::async_trait;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::ContextSource;
use domovoi_core::{Incident, IncidentStatus};

use super::{Maker, Spec, incident_of};
use crate::triage::TriageDeps;
use crate::triage::tools::history_verdict;

/// Similar incidents shown.
const SIMILAR: usize = 3;

/// Resolved incidents of the same host and check. They are history: what happened before is
/// evidence about the past, never permission.
#[derive(Clone)]
pub(crate) struct SimilarIncidentsSource {
    deps: TriageDeps,
    incident: Incident,
}

impl SimilarIncidentsSource {
    /// A source over the incident repository, looking for incidents like `incident`.
    pub(crate) fn new(deps: TriageDeps, incident: Incident) -> Self {
        Self { deps, incident }
    }
}

#[async_trait]
impl ContextSource for SimilarIncidentsSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("similar_incidents")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Experience
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "similar_incidents",
            ContextLayer::Experience,
            self.deps.redactor.as_ref(),
            query,
        );
        let Some((_, id)) = incident_of(query) else {
            return maker.gap_batch("this source needs an incident");
        };
        if self.incident.id() != id {
            return maker.gap_batch("this source is bound to another incident");
        }
        if maker.expired(self.deps.clock.now()) {
            return maker.gap_batch("timed out");
        }
        let incident = &self.incident;
        let text = incident
            .target()
            .map_or_else(|| incident.check().as_str(), |t| t.as_str());
        let Ok(found) = self
            .deps
            .incidents
            .search(text, Some(incident.host()), 20)
            .await
        else {
            return maker.gap_batch("similar incidents could not be searched");
        };
        let shown = usize::from(query.limit).min(SIMILAR);
        let items = found
            .iter()
            .filter(|i| {
                i.id() != incident.id()
                    && i.status() == IncidentStatus::Resolved
                    && i.check() == incident.check()
                    // Defence in depth: a repository that ignores the host argument must not
                    // leak another host's incident into this prompt.
                    && i.host() == incident.host()
            })
            .take(shown)
            .map(|i| {
                let verdict = history_verdict(i);
                maker.item(Spec {
                    key: &i.id().to_string(),
                    title: &format!("{}: {}", i.check(), i.title()),
                    body: &format!(
                        "- {} {}: {} ({verdict})",
                        i.opened_at(),
                        i.check(),
                        i.title()
                    ),
                    status: ItemStatus::Known,
                    origin: Origin::Historical,
                    reference: &format!("incident:{}", i.id()),
                    observed_at: Some(i.opened_at()),
                })
            })
            .collect();
        ContextBatch {
            items,
            gaps: Vec::new(),
        }
    }
}
