//! The incident so far, for a follow-up answer: its latest report and its recent
//! timeline.

use async_trait::async_trait;
use domovoi_core::TriageOutcome;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::{ContextSource, IncidentTimelineEntry};
use domovoi_core::triage::current_episode;

use super::{Maker, Spec, incident_of};
use crate::triage::{CLEAR_KIND, FOLLOW_UP_KIND, TriageDeps};

/// Most timeline lines listed, taking the newest.
const MAX_LINES: usize = 20;

/// The report and timeline of the incident. Follow-up entries are left out because
/// the conversation itself is sent back to the model separately.
#[derive(Clone)]
pub(crate) struct ThreadSource {
    deps: TriageDeps,
}

impl ThreadSource {
    /// A source over the incident repository.
    pub(crate) fn new(deps: TriageDeps) -> Self {
        Self { deps }
    }
}

/// The latest triage report of `timeline`, as the model is shown it; empty when the
/// incident has none.
fn report_body(timeline: &[IncidentTimelineEntry]) -> String {
    let outcome = timeline
        .iter()
        .rev()
        .filter(|e| e.kind.starts_with("triage.") && e.kind != FOLLOW_UP_KIND)
        .find_map(|e| serde_json::from_value(e.detail.clone()).ok());
    match outcome {
        Some(TriageOutcome::Report(found) | TriageOutcome::Cached { report: found, .. }) => {
            format!(
                "headline: {headline}\ncause: {cause}\nverdict: {verdict}\nsummary: {summary}\nnext step: {next_step}",
                headline = found.headline,
                cause = found.cause,
                verdict = found.verdict.label(),
                summary = found.summary,
                next_step = found.next_step,
            )
        }
        _ => String::new(),
    }
}

/// The newest twenty timeline lines that are not follow-up entries, oldest first, as
/// `kind: summary`. A cleared conversation must not resurface, so clears are skipped too.
fn timeline_body(timeline: &[IncidentTimelineEntry]) -> String {
    timeline
        .iter()
        .skip(timeline.len().saturating_sub(MAX_LINES))
        .filter(|entry| entry.kind != FOLLOW_UP_KIND && entry.kind != CLEAR_KIND)
        .map(|entry| format!("{}: {}", entry.kind, entry.summary))
        .collect::<Vec<String>>()
        .join("\n")
}

#[async_trait]
impl ContextSource for ThreadSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("incident_thread")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Incident
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "incident_thread",
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
        let mut batch = ContextBatch::default();
        let mut report = None;
        let mut lines = None;
        match self.deps.incidents.timeline(id).await {
            Err(_) => batch
                .gaps
                .push(maker.gap("the incident timeline could not be read")),
            Ok(timeline) => {
                let current = match self.deps.incidents.get(id).await {
                    Ok(Some(found)) => current_episode(found.check(), &timeline),
                    _ => &timeline[..],
                };
                report = Some(report_body(current)).filter(|text| !text.is_empty());
                lines = Some(timeline_body(&timeline)).filter(|text| !text.is_empty());
            }
        }
        for (key, title, origin, reference, body) in [
            (
                "report",
                "Latest triage report",
                Origin::Inferred,
                format!("triage report of incident:{id}"),
                report,
            ),
            (
                "timeline",
                "Timeline of this incident, oldest first",
                Origin::Observed,
                format!("timeline of incident:{id}"),
                lines,
            ),
        ] {
            if let Some(body) = body {
                batch.items.push(maker.item(Spec {
                    key,
                    title,
                    body: &body,
                    status: ItemStatus::Known,
                    origin,
                    reference: &reference,
                    observed_at: None,
                }));
            }
        }
        batch
    }
}
