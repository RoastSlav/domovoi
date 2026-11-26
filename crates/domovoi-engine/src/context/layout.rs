//! The shape of aggregated context: sections per source, the omissions caps caused, and the
//! heading each source's section carries.

use domovoi_core::context::{ContextGap, ContextItem, ContextLayer, ContextSourceId};

use super::plan::ContextPurpose;

/// What one source contributed: its items and what it could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The source.
    pub source: ContextSourceId,
    /// The layer of every item of the section.
    pub layer: ContextLayer,
    /// The heading the model reads.
    pub title: String,
    /// The items, in the order they are shown.
    pub items: Vec<ContextItem>,
    /// What the source could not answer.
    pub gaps: Vec<ContextGap>,
}

/// Items a cap or a limit left out of the context, counted per layer and source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omission {
    /// The layer of the dropped items.
    pub layer: ContextLayer,
    /// The source that produced them.
    pub source: ContextSourceId,
    /// How many were left out.
    pub items: u32,
}

/// The heading of the section a source's items appear under.
#[must_use]
pub fn title_of(source: &ContextSourceId, purpose: ContextPurpose, layer: ContextLayer) -> String {
    let fixed = match source.as_str() {
        "incident" => "Incident",
        "signals" => "Recent signals, newest first",
        "profile" => "Host profile (the normal state of this host)",
        "notes" => "Host notes",
        "changes" => "What changed in the last 24 hours",
        "similar_incidents" => "Similar resolved incidents",
        "skills" if purpose == ContextPurpose::Chat => "Read-only skills you may use on this host",
        "skills" => "Skills you may propose",
        "incident_thread" => "This incident so far: report and timeline",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_owned();
    }
    match layer {
        ContextLayer::Incident => format!("Incident: {source}"),
        ContextLayer::Host => format!("Host: {source}"),
        ContextLayer::Evidence => format!("Evidence: {source}"),
        ContextLayer::Experience => format!("Historical evidence, not authorization: {source}"),
        ContextLayer::Knowledge => format!("Knowledge: {source}"),
        ContextLayer::Capabilities => format!("Capabilities: {source}"),
    }
}
