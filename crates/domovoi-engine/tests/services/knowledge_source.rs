//! The knowledge layer of model context.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use domovoi_core::context::{
    ContextLayer, ContextQuery, ContextScope, ContextSelector, ItemStatus, Origin,
};
use domovoi_core::knowledge::{
    Entity, Fact, FactSource, KnowledgeStatus, PathConfirmation, Predicate, Relation, RelationKind,
    SubjectRef,
};
use domovoi_core::ports::{
    Clock, ContextSource, FactFilter, FactHit, KnowledgeRead, Page, Paged, RelationFilter,
    StoreError, Versioned,
};
use domovoi_core::{Actor, EntityId, FactRecordId, HostId, RelationId, UserId};
use domovoi_engine::knowledge::KnowledgeSource;
use domovoi_testkit::build::T0;
use domovoi_testkit::contracts::context::{ContextFixture, run as context_contract};
use domovoi_testkit::{FakeClock, ListRedactor};
use jiff::SignedDuration;
use serde_json::json;

use super::knowledge::{DAY, draft, owner_fact, rig, system};

const SECRET: &str = "hunter2-secret";

/// A read port that answers one canned fact, or fails.
struct Canned {
    host: HostId,
    fact: Mutex<Fact>,
    fail: AtomicBool,
}

impl Canned {
    fn new(host: HostId, now: jiff::Timestamp) -> Self {
        Self {
            host,
            fact: Mutex::new(fact_with(host, now, "start")),
            fail: AtomicBool::new(false),
        }
    }

    fn check(&self) -> Result<(), StoreError> {
        if self.fail.load(Ordering::SeqCst) {
            Err(StoreError::Backend("the store failed".into()))
        } else {
            Ok(())
        }
    }
}

fn fact_with(host: HostId, now: jiff::Timestamp, text: &str) -> Fact {
    Fact {
        id: FactRecordId::from_uuid(uuid::Uuid::from_u128(7)),
        subject: SubjectRef::Host(host),
        predicate: Predicate::OwnerNote,
        value: Some(json!(text)),
        source: FactSource::User,
        status: KnowledgeStatus::Current,
        observed_at: now,
        valid_until: None,
        last_verified_at: None,
        supersedes: None,
        evidence: None,
        actor: Actor::User(UserId::from_uuid(uuid::Uuid::from_u128(9))),
        created_at: now,
        retracted_at: None,
        retracted_by: None,
        content_purged_at: None,
    }
}

#[async_trait]
impl KnowledgeRead for Canned {
    async fn fact(&self, _: FactRecordId) -> Result<Option<Versioned<Fact>>, StoreError> {
        Ok(None)
    }

    async fn facts(&self, _: &FactFilter, _: &Page) -> Result<Paged<Fact>, StoreError> {
        self.check()?;
        let fact = self.fact.lock().map_err(|_| StoreError::NotFound)?.clone();
        Ok(Paged {
            items: vec![fact],
            next_cursor: None,
        })
    }

    async fn live_facts(
        &self,
        _: &SubjectRef,
        _: Predicate,
    ) -> Result<Vec<Versioned<Fact>>, StoreError> {
        Ok(Vec::new())
    }

    async fn all_live_facts(&self, _: u32) -> Result<Vec<Versioned<Fact>>, StoreError> {
        Ok(Vec::new())
    }

    async fn search_facts(
        &self,
        _: &str,
        _: Option<HostId>,
        _: u16,
    ) -> Result<Vec<FactHit>, StoreError> {
        self.check()?;
        let fact = self.fact.lock().map_err(|_| StoreError::NotFound)?.clone();
        Ok(vec![FactHit { fact, score: 1.0 }])
    }

    async fn confirmation(&self, _: FactRecordId) -> Result<Option<PathConfirmation>, StoreError> {
        Ok(None)
    }

    async fn relation(&self, _: RelationId) -> Result<Option<Versioned<Relation>>, StoreError> {
        Ok(None)
    }

    async fn relations(&self, _: &RelationFilter, _: &Page) -> Result<Paged<Relation>, StoreError> {
        self.check()?;
        Ok(Paged {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    async fn live_relation(
        &self,
        _: &SubjectRef,
        _: RelationKind,
        _: &SubjectRef,
    ) -> Result<Option<Versioned<Relation>>, StoreError> {
        Ok(None)
    }

    async fn all_live_relations(&self, _: u32) -> Result<Vec<Versioned<Relation>>, StoreError> {
        Ok(Vec::new())
    }

    async fn entity(&self, _: EntityId) -> Result<Option<Versioned<Entity>>, StoreError> {
        Ok(None)
    }

    async fn entities(&self) -> Result<Vec<Versioned<Entity>>, StoreError> {
        Ok(Vec::new())
    }

    async fn entity_in_use(&self, _: EntityId) -> Result<bool, StoreError> {
        Ok(false)
    }
}

struct Fixture {
    clock: Arc<FakeClock>,
    read: Arc<Canned>,
    source: KnowledgeSource,
    selector: ContextSelector,
}

impl Fixture {
    fn new(selector: ContextSelector) -> (Self, HostId) {
        let host = HostId::new();
        let clock = Arc::new(FakeClock::new(T0));
        let read = Arc::new(Canned::new(host, T0));
        let source = KnowledgeSource::new(
            read.clone(),
            Arc::new(ListRedactor::new([SECRET])),
            clock.clone(),
        );
        (
            Self {
                clock,
                read,
                source,
                selector,
            },
            host,
        )
    }
}

impl ContextFixture for Fixture {
    fn source(&self) -> &dyn ContextSource {
        &self.source
    }

    fn selector(&self) -> ContextSelector {
        self.selector.clone()
    }

    fn clock(&self) -> &FakeClock {
        &self.clock
    }

    fn secret(&self) -> &str {
        SECRET
    }

    fn answer_with(&self, text: &str) {
        self.read.fail.store(false, Ordering::SeqCst);
        if let Ok(mut fact) = self.read.fact.lock() {
            *fact = fact_with(self.read.host, T0, text);
        }
    }

    fn fail_next(&self) {
        self.read.fail.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn knowledge_source_meets_the_context_contract() {
    for selector in [
        ContextSelector::Overview,
        ContextSelector::Search {
            text: "note".to_owned(),
        },
    ] {
        let (fixture, host) = Fixture::new(selector);
        context_contract(&fixture, ContextScope::Host(host)).await;
    }
}

fn query(host: HostId, clock: &FakeClock, selector: ContextSelector) -> ContextQuery {
    ContextQuery {
        scope: ContextScope::Host(host),
        selector,
        limit: 20,
        max_item_bytes: 1024,
        deadline: clock.now() + SignedDuration::from_secs(30),
    }
}

#[tokio::test]
async fn knowledge_source_store_error_is_a_gap() {
    let (fixture, host) = Fixture::new(ContextSelector::Overview);
    fixture.fail_next();
    let batch = fixture
        .source
        .fetch(&query(host, &fixture.clock, ContextSelector::Overview))
        .await;
    assert!(batch.items.is_empty());
    assert_eq!(batch.gaps.len(), 1);
    assert_eq!(
        batch.gaps[0].reason,
        "the knowledge store could not be read"
    );
    assert!(fixture.source.supports(&ContextSelector::Overview));
    assert!(!fixture.source.supports(&ContextSelector::Read {
        call: domovoi_core::triage::ReadToolCall::HostOverview,
    }));
}

fn real_source(r: &super::knowledge::Rig) -> KnowledgeSource {
    KnowledgeSource::new(
        r.store.clone(),
        Arc::new(ListRedactor::new([SECRET])),
        r.clock.clone(),
    )
}

#[tokio::test]
async fn retracted_and_superseded_facts_never_reach_the_context() {
    let r = rig();
    let s = SubjectRef::Host(HostId::new());
    let host = s.host().unwrap();
    let kept = owner_fact(&r, &s, Predicate::Role, json!("kept role")).await;
    let gone = owner_fact(&r, &s, Predicate::OwnerNote, json!("retracted note")).await;
    r.service
        .retract(gone.fact.value.id, gone.fact.version, UserId::new())
        .await
        .unwrap();
    let old = r
        .service
        .record(
            draft(
                &s,
                Predicate::Criticality,
                json!("warning"),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    r.service
        .record(
            draft(
                &s,
                Predicate::Criticality,
                json!("critical"),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    let _ = old;
    let source = real_source(&r);
    for selector in [
        ContextSelector::Overview,
        ContextSelector::Search {
            text: "note".to_owned(),
        },
        ContextSelector::Search {
            text: "critical".to_owned(),
        },
    ] {
        let batch = source.fetch(&query(host, &r.clock, selector)).await;
        for item in &batch.items {
            assert!(!item.body.contains("retracted note"));
            assert!(item.body != "warning");
        }
    }
    let overview = source
        .fetch(&query(host, &r.clock, ContextSelector::Overview))
        .await;
    let bodies: Vec<&str> = overview.items.iter().map(|i| i.body.as_str()).collect();
    assert!(bodies.contains(&"kept role") && bodies.contains(&"critical"));
    assert!(
        overview
            .items
            .iter()
            .any(|i| i.id.as_str() == format!("knowledge:fact:{}", kept.fact.value.id))
    );
}

#[tokio::test]
async fn knowledge_section_renders_statuses() {
    let r = rig();
    let s = SubjectRef::Host(HostId::new());
    let host = s.host().unwrap();
    owner_fact(&r, &s, Predicate::OwnerNote, json!("old note")).await;
    r.clock.advance(SignedDuration::from_secs(400 * DAY));
    owner_fact(&r, &s, Predicate::Role, json!("media server")).await;
    r.service
        .record(
            draft(
                &s,
                Predicate::Criticality,
                json!("critical"),
                FactSource::Discovered,
            ),
            &system(),
        )
        .await
        .unwrap();
    r.clock.advance(SignedDuration::from_secs(1));
    r.service
        .record(
            draft(
                &s,
                Predicate::Criticality,
                json!("warning"),
                FactSource::Integration,
            ),
            &system(),
        )
        .await
        .unwrap();
    let source = real_source(&r);
    let batch = source
        .fetch(&query(host, &r.clock, ContextSelector::Overview))
        .await;
    assert!(batch.gaps.is_empty());
    let statuses: Vec<_> = batch.items.iter().map(|i| &i.status).collect();
    assert!(statuses.iter().any(|s| matches!(s, ItemStatus::Known)));
    assert!(
        statuses
            .iter()
            .any(|s| matches!(s, ItemStatus::Stale { .. }))
    );
    assert!(
        statuses
            .iter()
            .any(|s| matches!(s, ItemStatus::Disputed { .. }))
    );
    assert!(
        batch
            .items
            .iter()
            .all(|i| i.layer == ContextLayer::Knowledge)
    );
    assert!(
        batch
            .items
            .iter()
            .any(|i| i.provenance.origin == Origin::Declared)
    );
    assert!(
        batch
            .items
            .iter()
            .any(|i| i.provenance.origin == Origin::Observed)
    );
    let ids =
        regex::Regex::new("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
    let text = batch
        .items
        .iter()
        .map(|i| i.render())
        .collect::<Vec<_>>()
        .join("\n\n");
    insta::assert_snapshot!(ids.replace_all(&text, "<uuid>").to_string());
}
