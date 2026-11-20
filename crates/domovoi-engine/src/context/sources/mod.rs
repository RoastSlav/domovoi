//! The built-in context sources: the parts of the triage bundle and the follow-up thread,
//! each read-only, bounded and honest about what it could not read.

mod changes;
mod incident;
mod notes;
mod profile;
mod signals;
mod similar;
mod skills;
mod thread;

use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextItemId, ContextLayer, ContextQuery, ContextScope,
    ContextSourceId, ItemStatus, Origin, Provenance,
};
use domovoi_core::ports::Redactor;
use domovoi_core::{HostId, IncidentId};
use jiff::Timestamp;

pub(crate) use changes::ChangesSource;
pub(crate) use incident::IncidentSource;
pub(crate) use notes::NotesSource;
pub(crate) use profile::ProfileSource;
pub(crate) use signals::SignalsSource;
pub(crate) use similar::SimilarIncidentsSource;
pub(crate) use skills::SkillsSource;
pub(crate) use thread::ThreadSource;

use super::normalize;

/// The source id for a fixed literal name. The names used here are valid by construction (a
/// unit test parses each of them).
pub(super) fn source_id(name: &str) -> ContextSourceId {
    ContextSourceId::parse(name).unwrap_or_else(|_| ContextSourceId::evidence())
}

/// The incident a query is about, when its scope names one.
pub(super) fn incident_of(query: &ContextQuery) -> Option<(HostId, IncidentId)> {
    match query.scope {
        ContextScope::Incident { host, incident } => Some((host, incident)),
        _ => None,
    }
}

/// The host a query is about, whatever its scope.
pub(super) fn host_of(query: &ContextQuery) -> HostId {
    match query.scope {
        ContextScope::Host(host)
        | ContextScope::Incident { host, .. }
        | ContextScope::Operation { host, .. } => host,
    }
}

/// What one item says, before it is redacted and bounded.
pub(super) struct Spec<'a> {
    pub(super) key: &'a str,
    pub(super) title: &'a str,
    pub(super) body: &'a str,
    pub(super) status: ItemStatus,
    pub(super) origin: Origin,
    pub(super) reference: &'a str,
    pub(super) observed_at: Option<Timestamp>,
}

/// Builds the items and gaps of one source: redacted, bounded by the query and stamped with
/// the source's identity.
pub(super) struct Maker<'a> {
    pub(super) id: ContextSourceId,
    pub(super) layer: ContextLayer,
    redactor: &'a dyn Redactor,
    max: usize,
    deadline: Timestamp,
}

impl<'a> Maker<'a> {
    /// A maker for source `name` in `layer`, answering `query`.
    pub(super) fn new(
        name: &str,
        layer: ContextLayer,
        redactor: &'a dyn Redactor,
        query: &ContextQuery,
    ) -> Self {
        Self {
            id: source_id(name),
            layer,
            redactor,
            max: query.max_item_bytes as usize,
            deadline: query.deadline,
        }
    }

    /// Whether the query's deadline has passed at `now`.
    pub(super) fn expired(&self, now: Timestamp) -> bool {
        now >= self.deadline
    }

    /// A known item observed by Domovoi: every field redacted and the body cut to the
    /// query's bound.
    pub(super) fn item(&self, spec: Spec<'_>) -> ContextItem {
        let mut item = ContextItem {
            id: ContextItemId::new(&self.id, spec.key),
            source: self.id.clone(),
            layer: self.layer,
            title: spec.title.to_owned(),
            body: spec.body.trim().to_owned(),
            status: spec.status,
            provenance: Provenance {
                origin: spec.origin,
                reference: spec.reference.to_owned(),
                observed_at: spec.observed_at,
                actor: None,
            },
            truncated: false,
        };
        normalize::redact_and_bound(&mut item, self.redactor, self.max);
        item
    }

    /// An item for something that could not be read.
    pub(super) fn unknown(
        &self,
        key: &str,
        title: &str,
        reason: &str,
        reference: &str,
    ) -> ContextItem {
        self.item(Spec {
            key,
            title,
            body: "",
            status: ItemStatus::Unknown {
                reason: reason.to_owned(),
            },
            origin: Origin::Observed,
            reference,
            observed_at: None,
        })
    }

    /// A question the source could not answer.
    pub(super) fn gap(&self, reason: &str) -> ContextGap {
        let mut gap = ContextGap {
            source: self.id.clone(),
            reason: reason.to_owned(),
        };
        normalize::gap(&mut gap, &self.id, self.redactor);
        gap
    }

    /// A batch holding only a gap.
    pub(super) fn gap_batch(&self, reason: &str) -> ContextBatch {
        ContextBatch {
            items: Vec::new(),
            gaps: vec![self.gap(reason)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::source_id;

    #[test]
    fn every_built_in_source_name_is_a_valid_id() {
        for name in [
            "incident",
            "signals",
            "profile",
            "notes",
            "changes",
            "similar_incidents",
            "skills",
            "incident_thread",
        ] {
            assert_eq!(source_id(name).as_str(), name);
        }
    }
}
