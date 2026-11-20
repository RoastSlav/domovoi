//! The newest signals of the incident.

use std::fmt::Write as _;

use async_trait::async_trait;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::ContextSource;

use super::{Maker, Spec, incident_of};
use crate::triage::TriageDeps;

/// Most signals listed.
const SIGNALS: u16 = 10;

/// The recent signals of the incident, newest first, as one item.
#[derive(Clone)]
pub(crate) struct SignalsSource {
    deps: TriageDeps,
}

impl SignalsSource {
    /// A source over the signal repository.
    pub(crate) fn new(deps: TriageDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl ContextSource for SignalsSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("signals")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Incident
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "signals",
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
        let limit = query.limit.min(SIGNALS);
        let Ok(signals) = self.deps.signals.list_for_incident(id, limit).await else {
            return maker.gap_batch("the recent signals could not be read");
        };
        let Some(newest) = signals.first().map(|s| s.observed_at()) else {
            return ContextBatch::default();
        };
        let mut body = String::new();
        for signal in &signals {
            let evidence: Vec<String> = signal
                .evidence()
                .iter()
                .map(|(k, v)| format!("{k}={}", serde_json::to_string(v).unwrap_or_default()))
                .collect();
            let _ = writeln!(
                body,
                "- {} {:?} [{}] {} {{{}}}",
                signal.observed_at(),
                signal.status(),
                signal.severity(),
                signal.summary(),
                evidence.join(", ")
            );
        }
        ContextBatch {
            items: vec![maker.item(Spec {
                key: &id.to_string(),
                title: "Recent signals, newest first",
                body: &body,
                status: ItemStatus::Known,
                origin: Origin::Observed,
                reference: &format!("signals of incident:{id}"),
                observed_at: Some(newest),
            })],
            gaps: Vec::new(),
        }
    }
}
