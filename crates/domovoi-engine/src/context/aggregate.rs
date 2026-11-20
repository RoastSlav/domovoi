//! Asks every applicable source, normalises what comes back and applies the caps.

use std::sync::Arc;

use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextQuery, ContextSelector, ContextSourceId,
};
use domovoi_core::ports::{Clock, ContextSource, Redactor};

use super::layout::{Omission, Section, title_of};
use super::normalize;
use super::plan::ContextPlan;
use super::render::LayeredContext;
use super::shrink::shrink;

/// Builds [`LayeredContext`] from context sources.
///
/// It trusts no source: layer, source id, origin, redaction and bounds are re-checked on
/// everything a source returns. Sources are asked one after the other, in registration order.
pub struct Aggregator {
    sources: Vec<Arc<dyn ContextSource>>,
    redactor: Arc<dyn Redactor>,
    clock: Arc<dyn Clock>,
}

impl Aggregator {
    /// An aggregator over `sources`, asked in the order given.
    #[must_use]
    pub fn new(
        sources: Vec<Arc<dyn ContextSource>>,
        redactor: Arc<dyn Redactor>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            sources,
            redactor,
            clock,
        }
    }

    /// Gathers the context `plan` asks for. Never fails: a source that cannot answer becomes
    /// a gap in its section.
    pub async fn gather(&self, plan: &ContextPlan) -> LayeredContext {
        let mut sections: Vec<Section> = Vec::new();
        let mut omissions: Vec<Omission> = Vec::new();
        for source in &self.sources {
            let mut selectors = Vec::new();
            for (selector, limit) in [
                (ContextSelector::Overview, plan.budget.overview_limit),
                (
                    ContextSelector::Search {
                        text: plan.search_text.clone().unwrap_or_default(),
                    },
                    plan.budget.search_limit,
                ),
            ] {
                let wanted = match &selector {
                    ContextSelector::Search { text } => !text.is_empty(),
                    _ => true,
                };
                if wanted && source.supports(&selector) {
                    selectors.push((selector, limit));
                }
            }
            if selectors.is_empty() {
                continue;
            }
            let id = source.id();
            let layer = source.layer();
            let mut items: Vec<ContextItem> = Vec::new();
            let mut gaps: Vec<ContextGap> = Vec::new();
            let mut left_out = 0u32;
            for (selector, limit) in selectors {
                let batch = self.ask(source, plan, selector, limit).await;
                let mut returned = batch.items;
                if returned.len() > usize::from(limit) {
                    let extra = returned.len() - usize::from(limit);
                    left_out += u32::try_from(extra).unwrap_or(u32::MAX);
                    tracing::info!(source = %id, count = extra, "a source returned more items than asked for");
                    returned.truncate(usize::from(limit));
                }
                for mut item in returned {
                    if normalize::restamp(&mut item, &id, layer) {
                        tracing::warn!(source = %id, "a source returned an item naming another source or layer");
                    }
                    normalize::force_origin(&mut item);
                    normalize::redact_and_bound(
                        &mut item,
                        self.redactor.as_ref(),
                        plan.budget.item_bytes as usize,
                    );
                    items.push(item);
                }
                for mut gap in batch.gaps {
                    normalize::gap(&mut gap, &id, self.redactor.as_ref());
                    gaps.push(gap);
                }
            }
            if left_out > 0 {
                omissions.push(Omission {
                    layer,
                    source: id.clone(),
                    items: left_out,
                });
            }
            sections.push(Section {
                title: title_of(&id, plan.purpose, layer),
                source: id,
                layer,
                items: normalize::dedupe(items),
                gaps,
            });
        }
        sections.sort_by_key(|s| s.layer);
        let (sections, omissions) = shrink(
            sections,
            omissions,
            plan.budget.layers,
            plan.budget.total_bytes,
        );
        for omission in &omissions {
            tracing::info!(
                layer = omission.layer.as_str(),
                source = %omission.source,
                count = omission.items,
                "context left out to fit the size limit",
            );
        }
        LayeredContext {
            sections,
            omissions,
        }
    }

    /// One query to one source, bounded by the source deadline.
    async fn ask(
        &self,
        source: &Arc<dyn ContextSource>,
        plan: &ContextPlan,
        selector: ContextSelector,
        limit: u16,
    ) -> ContextBatch {
        let now = self.clock.now();
        let deadline = now.checked_add(plan.budget.source_deadline).unwrap_or(now);
        let query = ContextQuery {
            scope: plan.scope.clone(),
            selector,
            limit,
            max_item_bytes: plan.budget.item_bytes,
            deadline,
        };
        let timed_out = |id: ContextSourceId| ContextBatch {
            items: Vec::new(),
            gaps: vec![ContextGap {
                source: id,
                reason: "timed out".to_owned(),
            }],
        };
        let remaining = deadline.duration_since(now);
        let Ok(wait) = std::time::Duration::try_from(remaining) else {
            return timed_out(source.id());
        };
        if wait.is_zero() {
            return timed_out(source.id());
        }
        match tokio::time::timeout(wait, source.fetch(&query)).await {
            Ok(batch) => batch,
            Err(_) => timed_out(source.id()),
        }
    }
}
