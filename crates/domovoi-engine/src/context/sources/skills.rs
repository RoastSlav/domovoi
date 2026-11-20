//! The skills the model may propose (or, in a chat, use) on the host.

use std::fmt::Write as _;

use async_trait::async_trait;
use domovoi_core::SkillSummary;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::ContextSource;

use super::{Maker, Spec, host_of};
use crate::triage::TriageDeps;

/// The skill catalogue of one host. When the host's agent is not connected its skills are
/// not known, and the source says so instead of listing none.
#[derive(Clone)]
pub(crate) struct SkillsSource {
    deps: TriageDeps,
    skills: Vec<SkillSummary>,
    connected: bool,
}

impl SkillsSource {
    /// A source over `skills`, the catalogue the caller already read; `connected` says
    /// whether the host's agent was connected when it did.
    pub(crate) fn new(deps: TriageDeps, skills: Vec<SkillSummary>, connected: bool) -> Self {
        Self {
            deps,
            skills,
            connected,
        }
    }
}

/// The skill catalogue as the model reads it: one line per skill with its risk, description
/// and parameters.
pub(crate) fn catalogue(skills: &[SkillSummary]) -> String {
    let mut out = String::new();
    for skill in skills {
        let params = serde_json::to_string(&skill.params).unwrap_or_default();
        let applies: Vec<&str> = skill.applies_to.iter().map(|c| c.as_str()).collect();
        let _ = writeln!(
            out,
            "- {} (risk: {:?}): {} Parameters: {params}.{}",
            skill.name,
            skill.risk,
            skill.description.trim(),
            if applies.is_empty() {
                String::new()
            } else {
                format!(" Only for checks: {}.", applies.join(", "))
            }
        );
    }
    out
}

#[async_trait]
impl ContextSource for SkillsSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("skills")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Capabilities
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "skills",
            ContextLayer::Capabilities,
            self.deps.redactor.as_ref(),
            query,
        );
        if maker.expired(self.deps.clock.now()) {
            return maker.gap_batch("timed out");
        }
        let host = host_of(query);
        let reference = format!("skill manifests of host:{host}");
        if !self.connected {
            return ContextBatch {
                items: vec![maker.unknown(
                    "catalogue",
                    "Skills",
                    "the host's agent is not connected, so its skills are not known",
                    &reference,
                )],
                gaps: Vec::new(),
            };
        }
        if self.skills.is_empty() {
            return ContextBatch::default();
        }
        ContextBatch {
            items: vec![maker.item(Spec {
                key: "catalogue",
                title: "Skills",
                body: &catalogue(&self.skills),
                status: ItemStatus::Known,
                origin: Origin::Declared,
                reference: &reference,
                observed_at: None,
            })],
            gaps: Vec::new(),
        }
    }
}
