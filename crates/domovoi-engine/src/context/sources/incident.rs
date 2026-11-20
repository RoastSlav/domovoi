//! The incident itself, as the model first reads it.

use std::fmt::Write as _;

use async_trait::async_trait;
use domovoi_core::Incident;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::ContextSource;

use super::{Maker, Spec, incident_of};
use crate::triage::TriageDeps;

/// The incident the scope names: title, check, target, severity, status and counts.
#[derive(Clone)]
pub(crate) struct IncidentSource {
    deps: TriageDeps,
}

impl IncidentSource {
    /// A source over the incident repository.
    pub(crate) fn new(deps: TriageDeps) -> Self {
        Self { deps }
    }
}

/// The lines the model reads about `incident`.
pub(crate) fn snapshot(incident: &Incident) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Title: {}", incident.title());
    let _ = writeln!(out, "Check: {}", incident.check());
    if let Some(target) = incident.target() {
        let _ = writeln!(out, "Target: {target}");
    }
    let _ = writeln!(out, "Host id: {}", incident.host());
    let _ = writeln!(out, "Severity: {}", incident.severity());
    let _ = writeln!(out, "Status: {:?}", incident.status());
    let _ = writeln!(out, "Opened: {}", incident.opened_at());
    let _ = writeln!(out, "Last firing: {}", incident.last_firing_at());
    let _ = writeln!(out, "Firing signals so far: {}", incident.occurrences());
    let history = incident.history();
    if let Some((at, value)) = history.latest() {
        let _ = writeln!(
            out,
            "Tracked value: {value} at {at} ({} samples)",
            history.len()
        );
    }
    out
}

#[async_trait]
impl ContextSource for IncidentSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("incident")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Incident
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "incident",
            ContextLayer::Incident,
            self.deps.redactor.as_ref(),
            query,
        );
        let Some((_, id)) = incident_of(query) else {
            return maker.gap_batch("this source needs an incident");
        };
        if maker.expired(self.deps.clock.now()) {
            return maker.gap_batch("timed out");
        }
        match self.deps.incidents.get(id).await {
            Err(_) => maker.gap_batch("the incident could not be read"),
            Ok(None) => maker.gap_batch("the incident no longer exists"),
            Ok(Some(incident)) => ContextBatch {
                items: vec![maker.item(Spec {
                    key: &id.to_string(),
                    title: "Incident",
                    body: &snapshot(&incident),
                    status: ItemStatus::Known,
                    origin: Origin::Observed,
                    reference: &format!("incident:{id}"),
                    observed_at: Some(incident.last_firing_at()),
                })],
                gaps: Vec::new(),
            },
        }
    }
}
