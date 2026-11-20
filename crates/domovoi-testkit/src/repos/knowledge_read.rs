//! `KnowledgeRead` for the in-memory store.

use async_trait::async_trait;
use domovoi_core::knowledge::{
    Entity, Fact, KnowledgeStatus, PathConfirmation, Predicate, Relation, RelationKind, SubjectRef,
    canonical_json,
};
use domovoi_core::ports::{
    FactFilter, FactHit, KnowledgeRead, Page, Paged, RelationFilter, StoreError, Versioned,
};
use domovoi_core::{EntityId, FactRecordId, HostId, RelationId};

use super::knowledge_apply::relation_live;
use super::{InMemoryStore, Key, limit, paginate, word_score, words};

fn key(at: jiff::Timestamp, id: impl ToString) -> Key {
    (at.as_nanosecond(), id.to_string())
}

#[async_trait]
impl KnowledgeRead for InMemoryStore {
    async fn fact(&self, id: FactRecordId) -> Result<Option<Versioned<Fact>>, StoreError> {
        let state = self.begin()?;
        Ok(state.knowledge.facts.get(&id).map(|(f, v)| Versioned {
            value: f.clone(),
            version: *v,
        }))
    }

    async fn facts(&self, filter: &FactFilter, page: &Page) -> Result<Paged<Fact>, StoreError> {
        let state = self.begin()?;
        let items: Vec<(Key, Fact)> = state
            .knowledge
            .facts
            .values()
            .map(|(f, _)| f)
            .filter(|f| filter.host.is_none_or(|h| f.subject.host() == Some(h)))
            .filter(|f| filter.subject.as_ref().is_none_or(|s| &f.subject == s))
            .filter(|f| filter.predicate.is_none_or(|p| f.predicate == p))
            .filter(|f| filter.source.is_none_or(|s| f.source == s))
            .filter(|f| match &filter.statuses {
                None => f.status.is_live(),
                Some(list) => list.contains(&f.status),
            })
            .map(|f| (key(f.created_at, f.id), f.clone()))
            .collect();
        paginate(items, page)
    }

    async fn live_facts(
        &self,
        subject: &SubjectRef,
        predicate: Predicate,
    ) -> Result<Vec<Versioned<Fact>>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<Versioned<Fact>> = state
            .knowledge
            .facts
            .values()
            .filter(|(f, _)| {
                f.status.is_live() && &f.subject == subject && f.predicate == predicate
            })
            .map(|(f, v)| Versioned {
                value: f.clone(),
                version: *v,
            })
            .collect();
        rows.sort_by_key(|r| key(r.value.created_at, r.value.id));
        Ok(rows)
    }

    async fn all_live_facts(&self, max: u32) -> Result<Vec<Versioned<Fact>>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<Versioned<Fact>> = state
            .knowledge
            .facts
            .values()
            .filter(|(f, _)| f.status.is_live())
            .map(|(f, v)| Versioned {
                value: f.clone(),
                version: *v,
            })
            .collect();
        rows.sort_by_key(|r| key(r.value.created_at, r.value.id));
        rows.truncate(usize::try_from(max).unwrap_or(usize::MAX));
        Ok(rows)
    }

    async fn search_facts(
        &self,
        query: &str,
        host: Option<HostId>,
        max: u16,
    ) -> Result<Vec<FactHit>, StoreError> {
        let query = words(query);
        let state = self.begin()?;
        let mut hits: Vec<FactHit> = state
            .knowledge
            .facts
            .values()
            .map(|(f, _)| f)
            .filter(|f| f.status.is_live() && f.value.is_some())
            .filter(|f| match (host, f.subject.host()) {
                (Some(h), Some(fh)) => h == fh,
                _ => true,
            })
            .filter_map(|f| {
                let text = f.value.as_ref().map(domovoi_core::knowledge::search_text)?;
                let mut hay = words(&f.subject.to_string());
                hay.extend(words(f.predicate.as_str()));
                hay.extend(words(&text));
                let score = word_score(&query, &hay)?;
                Some(FactHit {
                    fact: f.clone(),
                    score: score as f64,
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| b.fact.created_at.cmp(&a.fact.created_at))
        });
        hits.truncate(limit(max));
        Ok(hits)
    }

    async fn confirmation(
        &self,
        fact: FactRecordId,
    ) -> Result<Option<PathConfirmation>, StoreError> {
        let state = self.begin()?;
        Ok(state.knowledge.confirmations.get(&fact).cloned())
    }

    async fn relation(&self, id: RelationId) -> Result<Option<Versioned<Relation>>, StoreError> {
        let state = self.begin()?;
        Ok(state.knowledge.relations.get(&id).map(|(r, v)| Versioned {
            value: r.clone(),
            version: *v,
        }))
    }

    async fn relations(
        &self,
        filter: &RelationFilter,
        page: &Page,
    ) -> Result<Paged<Relation>, StoreError> {
        let state = self.begin()?;
        let items: Vec<(Key, Relation)> = state
            .knowledge
            .relations
            .values()
            .map(|(r, _)| r)
            .filter(|r| {
                filter
                    .subject
                    .as_ref()
                    .is_none_or(|s| r.from.as_ref() == Some(s) || r.to.as_ref() == Some(s))
            })
            .filter(|r| filter.kind.is_none_or(|k| r.kind == k))
            .filter(|r| filter.include_retracted || r.status != KnowledgeStatus::Retracted)
            .map(|r| (key(r.created_at, r.id), r.clone()))
            .collect();
        paginate(items, page)
    }

    async fn live_relation(
        &self,
        from: &SubjectRef,
        kind: RelationKind,
        to: &SubjectRef,
    ) -> Result<Option<Versioned<Relation>>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .knowledge
            .relations
            .values()
            .find(|(r, _)| {
                relation_live(r)
                    && r.from.as_ref() == Some(from)
                    && r.kind == kind
                    && r.to.as_ref() == Some(to)
            })
            .map(|(r, v)| Versioned {
                value: r.clone(),
                version: *v,
            }))
    }

    async fn all_live_relations(&self, max: u32) -> Result<Vec<Versioned<Relation>>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<Versioned<Relation>> = state
            .knowledge
            .relations
            .values()
            .filter(|(r, _)| relation_live(r))
            .map(|(r, v)| Versioned {
                value: r.clone(),
                version: *v,
            })
            .collect();
        rows.sort_by_key(|r| key(r.value.created_at, r.value.id));
        rows.truncate(usize::try_from(max).unwrap_or(usize::MAX));
        Ok(rows)
    }

    async fn entity(&self, id: EntityId) -> Result<Option<Versioned<Entity>>, StoreError> {
        let state = self.begin()?;
        Ok(state.knowledge.entities.get(&id).map(|(e, v)| Versioned {
            value: e.clone(),
            version: *v,
        }))
    }

    async fn entities(&self) -> Result<Vec<Versioned<Entity>>, StoreError> {
        let state = self.begin()?;
        let mut rows: Vec<Versioned<Entity>> = state
            .knowledge
            .entities
            .values()
            .map(|(e, v)| Versioned {
                value: e.clone(),
                version: *v,
            })
            .collect();
        rows.sort_by_key(|r| r.value.name.to_lowercase());
        Ok(rows)
    }

    async fn entity_in_use(&self, id: EntityId) -> Result<bool, StoreError> {
        let state = self.begin()?;
        let subject = SubjectRef::Entity(id);
        let text = subject.to_string();
        let in_fact = state.knowledge.facts.values().any(|(f, _)| {
            f.status.is_live()
                && (f.subject == subject
                    || (f.predicate == Predicate::DependsOn
                        && f.value.as_ref().is_some_and(|v| {
                            canonical_json(v) == canonical_json(&serde_json::json!(text))
                        })))
        });
        let in_relation = state.knowledge.relations.values().any(|(r, _)| {
            relation_live(r)
                && (r.from.as_ref() == Some(&subject) || r.to.as_ref() == Some(&subject))
        });
        Ok(in_fact || in_relation)
    }
}
