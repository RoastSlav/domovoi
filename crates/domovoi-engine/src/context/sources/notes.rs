//! Notes the owner wrote about the host that match the incident.

use async_trait::async_trait;
use domovoi_core::Incident;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::ContextSource;

use super::{Maker, Spec, incident_of};
use crate::triage::TriageDeps;

/// Notes shown.
const NOTES: usize = 3;

/// The search asks for more hits than are shown, so the host's own profile note, which
/// carries the same common words, cannot take a shown place and push a runbook note out.
const NOTE_HITS: u16 = 8;

/// Host notes that match the incident's check and target.
#[derive(Clone)]
pub(crate) struct NotesSource {
    deps: TriageDeps,
    incident: Incident,
}

impl NotesSource {
    /// A source over the note repository, searching for notes about `incident`.
    pub(crate) fn new(deps: TriageDeps, incident: Incident) -> Self {
        Self { deps, incident }
    }
}

#[async_trait]
impl ContextSource for NotesSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("notes")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Host
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "notes",
            ContextLayer::Host,
            self.deps.redactor.as_ref(),
            query,
        );
        let Some((host, id)) = incident_of(query) else {
            return maker.gap_batch("this source needs an incident");
        };
        if maker.expired(self.deps.clock.now()) {
            return maker.gap_batch("timed out");
        }
        if self.incident.id() != id {
            return maker.gap_batch("this source is bound to another incident");
        }
        let incident = &self.incident;
        let mut text = incident.check().as_str().replace(['.', '_'], " ");
        if let Some(target) = incident.target() {
            text.push(' ');
            text.push_str(target.as_str());
        }
        let Ok(hits) = self.deps.notes.search(&text, Some(host), NOTE_HITS).await else {
            return maker.gap_batch("the host notes could not be searched");
        };
        let shown = usize::from(query.limit).min(NOTES);
        let items = hits
            .iter()
            .take(shown)
            .map(|h| {
                maker.item(Spec {
                    key: &h.note.id.to_string(),
                    title: &h.note.title,
                    body: &h.snippet,
                    status: ItemStatus::Known,
                    origin: Origin::Declared,
                    reference: &format!("note:{}", h.note.id),
                    observed_at: Some(h.note.updated_at),
                })
            })
            .collect();
        ContextBatch {
            items,
            gaps: Vec::new(),
        }
    }
}
