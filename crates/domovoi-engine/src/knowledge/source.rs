//! The knowledge layer of model context: what the owner and Domovoi have recorded about a host.
//!
//! Facts and relations are evidence, never permission: nothing in an item says a fact may be
//! relied on for safety. Superseded and retracted facts are never returned.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::context::{
    ContextBatch, ContextGap, ContextItem, ContextItemId, ContextLayer, ContextQuery, ContextScope,
    ContextSelector, ContextSourceId, ItemStatus, Origin, Provenance,
};
use domovoi_core::knowledge::{
    Fact, FactSource, KnowledgeStatus, Relation, SubjectRef, canonical_json,
};
use domovoi_core::ports::{
    Clock, ContextSource, FactFilter, KnowledgeRead, Page, Redactor, RelationFilter,
};
use jiff::Timestamp;

/// Most items an overview returns.
const OVERVIEW_ITEMS: usize = 20;
/// Longest title and reference, in characters.
const TITLE_CHARS: usize = 120;
/// Longest gap reason, in characters.
const REASON_CHARS: usize = 200;

/// What one item says, before it is redacted and bounded.
struct Spec<'a> {
    key: String,
    title: &'a str,
    body: &'a str,
    status: ItemStatus,
    origin: Origin,
    observed_at: Timestamp,
    actor: &'a str,
}

/// Context from the knowledge store.
#[derive(Clone)]
pub struct KnowledgeSource {
    read: Arc<dyn KnowledgeRead>,
    redactor: Arc<dyn Redactor>,
    clock: Arc<dyn Clock>,
}

impl KnowledgeSource {
    /// A source over the knowledge store. The clock decides freshness and the query deadline.
    #[must_use]
    pub fn new(
        read: Arc<dyn KnowledgeRead>,
        redactor: Arc<dyn Redactor>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            read,
            redactor,
            clock,
        }
    }

    fn id() -> ContextSourceId {
        ContextSourceId::parse("knowledge").unwrap_or_else(|_| ContextSourceId::evidence())
    }

    fn gap(&self, reason: &str) -> ContextBatch {
        ContextBatch {
            items: Vec::new(),
            gaps: vec![ContextGap {
                source: Self::id(),
                reason: self
                    .redactor
                    .redact(reason)
                    .chars()
                    .take(REASON_CHARS)
                    .collect(),
            }],
        }
    }

    fn cut(&self, text: &str, max_chars: usize) -> String {
        self.redactor.redact(text).chars().take(max_chars).collect()
    }

    /// One item: redacted, and the body cut to the query's bound.
    fn item(&self, spec: Spec<'_>, max_bytes: usize) -> ContextItem {
        let Spec {
            key,
            title,
            body,
            status,
            origin,
            observed_at,
            actor,
        } = spec;
        let source = Self::id();
        let redacted = self.redactor.redact(body);
        let mut end = redacted.len().min(max_bytes);
        while !redacted.is_char_boundary(end) {
            end -= 1;
        }
        let title = self.cut(title, TITLE_CHARS);
        ContextItem {
            id: ContextItemId::new(&source, &key),
            source: source.clone(),
            layer: ContextLayer::Knowledge,
            title: if title.trim().is_empty() {
                source.to_string()
            } else {
                title
            },
            body: redacted[..end].to_owned(),
            status,
            provenance: Provenance {
                origin,
                reference: self.cut(&key, TITLE_CHARS),
                observed_at: Some(observed_at),
                actor: Some(self.cut(actor, TITLE_CHARS)),
            },
            truncated: end < redacted.len(),
        }
    }

    async fn fact_item(
        &self,
        fact: &Fact,
        now: Timestamp,
        max_bytes: usize,
    ) -> Option<ContextItem> {
        let effective = fact.effective_status(now);
        let status = match effective {
            KnowledgeStatus::Current => ItemStatus::Known,
            KnowledgeStatus::Stale => ItemStatus::Stale {
                observed_at: fact.last_verified_at.unwrap_or(fact.observed_at),
            },
            KnowledgeStatus::Disputed => {
                let other = self
                    .read
                    .live_facts(&fact.subject, fact.predicate)
                    .await
                    .ok()
                    .and_then(|all| all.into_iter().find(|v| v.value.id != fact.id));
                match other {
                    Some(other) => ItemStatus::Disputed {
                        with: ContextItemId::new(&Self::id(), &format!("fact:{}", other.value.id)),
                    },
                    None => ItemStatus::Known,
                }
            }
            KnowledgeStatus::Superseded | KnowledgeStatus::Retracted => return None,
        };
        let value = fact.value.as_ref()?;
        let body = match value.as_str() {
            Some(text) => text.to_owned(),
            None => canonical_json(value),
        };
        let origin = match fact.source {
            FactSource::User | FactSource::Imported => Origin::Declared,
            FactSource::Discovered | FactSource::Integration => Origin::Observed,
            FactSource::Inferred => Origin::Inferred,
        };
        Some(self.item(
            Spec {
                key: format!("fact:{}", fact.id),
                title: &format!("{} of {}", fact.predicate.as_str(), fact.subject),
                body: &body,
                status,
                origin,
                observed_at: fact.observed_at,
                actor: &fact.actor.to_string(),
            },
            max_bytes,
        ))
    }

    fn relation_item(
        &self,
        relation: &Relation,
        now: Timestamp,
        max_bytes: usize,
    ) -> Option<ContextItem> {
        let (from, to) = (relation.from.as_ref()?, relation.to.as_ref()?);
        let status = match relation.effective_status(now) {
            KnowledgeStatus::Current => ItemStatus::Known,
            KnowledgeStatus::Stale => ItemStatus::Stale {
                observed_at: relation.observed_at,
            },
            _ => return None,
        };
        let origin = match relation.source {
            FactSource::User | FactSource::Imported => Origin::Declared,
            FactSource::Discovered | FactSource::Integration => Origin::Observed,
            FactSource::Inferred => Origin::Inferred,
        };
        let text = format!("{from} {} {to}", relation.kind.as_str());
        Some(self.item(
            Spec {
                key: format!("relation:{}", relation.id),
                title: &format!("{} relation of {from}", relation.kind.as_str()),
                body: &text,
                status,
                origin,
                observed_at: relation.observed_at,
                actor: &relation.actor.to_string(),
            },
            max_bytes,
        ))
    }

    async fn overview(&self, host: HostId, query: &ContextQuery, now: Timestamp) -> ContextBatch {
        let max_bytes = query.max_item_bytes as usize;
        let shown = usize::from(query.limit).min(OVERVIEW_ITEMS);
        let filter = FactFilter {
            host: Some(host),
            ..FactFilter::default()
        };
        let Ok(facts) = self.read.facts(&filter, &Page::first(50)).await else {
            return self.gap("the knowledge store could not be read");
        };
        let relation_filter = RelationFilter {
            subject: Some(SubjectRef::Host(host)),
            ..RelationFilter::default()
        };
        let Ok(relations) = self
            .read
            .relations(&relation_filter, &Page::first(50))
            .await
        else {
            return self.gap("the knowledge store could not be read");
        };
        let mut facts: Vec<Fact> = facts.items;
        facts.sort_by_key(|f| {
            let owner_current =
                f.source == FactSource::User && f.effective_status(now) == KnowledgeStatus::Current;
            (
                !owner_current,
                f.predicate.as_str(),
                std::cmp::Reverse(f.created_at),
            )
        });
        let mut items = Vec::new();
        for fact in &facts {
            if items.len() >= shown {
                break;
            }
            if let Some(item) = self.fact_item(fact, now, max_bytes).await {
                items.push(item);
            }
        }
        let mut relations = relations.items;
        relations.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        for relation in &relations {
            if items.len() >= shown {
                break;
            }
            if let Some(item) = self.relation_item(relation, now, max_bytes) {
                items.push(item);
            }
        }
        ContextBatch {
            items,
            gaps: Vec::new(),
        }
    }
}

fn host_of(scope: &ContextScope) -> HostId {
    match scope {
        ContextScope::Host(host)
        | ContextScope::Incident { host, .. }
        | ContextScope::Operation { host, .. } => *host,
    }
}

#[async_trait]
impl ContextSource for KnowledgeSource {
    fn id(&self) -> ContextSourceId {
        Self::id()
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Knowledge
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(
            selector,
            ContextSelector::Overview | ContextSelector::Search { .. }
        )
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let now = self.clock.now();
        if now >= query.deadline {
            return self.gap("timed out");
        }
        let host = host_of(&query.scope);
        match &query.selector {
            ContextSelector::Overview => self.overview(host, query, now).await,
            ContextSelector::Search { text } => {
                let max_bytes = query.max_item_bytes as usize;
                let Ok(hits) = self.read.search_facts(text, Some(host), query.limit).await else {
                    return self.gap("the knowledge store could not be searched");
                };
                let mut items = Vec::new();
                for hit in &hits {
                    if items.len() >= usize::from(query.limit) {
                        break;
                    }
                    if let Some(item) = self.fact_item(&hit.fact, now, max_bytes).await {
                        items.push(item);
                    }
                }
                ContextBatch {
                    items,
                    gaps: Vec::new(),
                }
            }
            ContextSelector::Read { .. } => self.gap("this source does not answer exact reads"),
        }
    }
}
