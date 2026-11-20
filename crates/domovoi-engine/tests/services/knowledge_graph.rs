//! Relations and entities.

use domovoi_core::knowledge::{FactSource, KnowledgeStatus, Predicate, RelationKind, SubjectRef};
use domovoi_core::ports::{JournalRead, KnowledgeRead, Page, RelationFilter};
use domovoi_core::{EntityId, UserId};
use domovoi_engine::knowledge::{KnowledgeError, RelationDraft};
use jiff::SignedDuration;
use serde_json::json;

use super::knowledge::{DAY, host, owner_fact, rig, system};

fn edge(from: &SubjectRef, to: &SubjectRef, source: FactSource) -> RelationDraft {
    RelationDraft {
        from: from.clone(),
        kind: RelationKind::RunsOn,
        to: to.clone(),
        source,
        observed_at: None,
        evidence: None,
    }
}

#[tokio::test]
async fn entity_names_are_unique_ignoring_case() {
    let r = rig();
    let user = UserId::new();
    r.service.create_entity("vpn", "VPN", user).await.unwrap();
    let err = r
        .service
        .create_entity("vpn", "vpn", user)
        .await
        .unwrap_err();
    assert_eq!(err, KnowledgeError::DuplicateEntity);
}

#[tokio::test]
async fn an_entity_in_use_cannot_be_deleted() {
    let r = rig();
    let user = UserId::new();
    let a = r.service.create_entity("vpn", "A", user).await.unwrap();
    let b = r.service.create_entity("vpn", "B", user).await.unwrap();
    let c = r.service.create_entity("vpn", "C", user).await.unwrap();
    let fact = owner_fact(
        &r,
        &SubjectRef::Entity(a.value.id),
        Predicate::Role,
        json!("x"),
    )
    .await;
    let rel = r
        .service
        .assert_relation(
            edge(&host(), &SubjectRef::Entity(b.value.id), FactSource::User),
            user,
        )
        .await
        .unwrap();
    let dep = owner_fact(
        &r,
        &host(),
        Predicate::DependsOn,
        json!(SubjectRef::Entity(c.value.id).to_string()),
    )
    .await;
    for e in [&a, &b, &c] {
        let err = r
            .service
            .delete_entity(e.value.id, e.version, user)
            .await
            .unwrap_err();
        assert_eq!(err, KnowledgeError::EntityInUse);
    }
    r.service
        .retract(fact.fact.value.id, fact.fact.version, user)
        .await
        .unwrap();
    r.service
        .retract_relation(rel.value.id, rel.version, user)
        .await
        .unwrap();
    r.service
        .retract(dep.fact.value.id, dep.fact.version, user)
        .await
        .unwrap();
    for e in [&a, &b, &c] {
        r.service
            .delete_entity(e.value.id, e.version, user)
            .await
            .unwrap();
    }
    assert!(r.store.entities().await.unwrap().is_empty());
}

#[tokio::test]
async fn only_a_user_creates_entities() {
    let r = rig();
    let user = UserId::new();
    let e = r
        .service
        .create_entity("nas-pool", "Pool", user)
        .await
        .unwrap();
    assert_eq!(e.value.created_by, domovoi_core::Actor::User(user));
    assert!(
        r.service
            .create_entity("Bad Kind", "x", user)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn relation_record_is_idempotent() {
    let r = rig();
    let (a, b) = (host(), host());
    let first = r
        .service
        .record_relation(edge(&a, &b, FactSource::Discovered), &system())
        .await
        .unwrap();
    r.clock.advance(SignedDuration::from_secs(DAY));
    let mut again = edge(&a, &b, FactSource::Discovered);
    again.observed_at = Some(r.clock_now());
    let second = r.service.record_relation(again, &system()).await.unwrap();
    assert_eq!(first.value.id, second.value.id);
    assert_eq!(second.value.observed_at, r.clock_now());
    let page = r
        .store
        .relations(&RelationFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    let changes: Vec<String> = r
        .store
        .entries_after(0, 100)
        .await
        .unwrap()
        .iter()
        .filter(|e| e.kind == "relation.changed")
        .map(|e| e.payload["change"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(changes, ["recorded", "refreshed"]);
}

#[tokio::test]
async fn relation_to_self_is_refused() {
    let r = rig();
    let a = host();
    let err = r
        .service
        .record_relation(edge(&a, &a, FactSource::Discovered), &system())
        .await
        .unwrap_err();
    assert!(matches!(err, KnowledgeError::InvalidInput(_)));
    let user_err = r
        .service
        .record_relation(edge(&a, &host(), FactSource::User), &system())
        .await
        .unwrap_err();
    assert!(matches!(user_err, KnowledgeError::ActorNotAllowed { .. }));
}

#[tokio::test]
async fn retracted_relation_is_not_listed_by_default() {
    let r = rig();
    let user = UserId::new();
    let rel = r
        .service
        .assert_relation(edge(&host(), &host(), FactSource::Inferred), user)
        .await
        .unwrap();
    assert_eq!(rel.value.source, FactSource::User);
    r.service
        .retract_relation(rel.value.id, rel.version, user)
        .await
        .unwrap();
    let page = Page::first(10);
    let live = r
        .store
        .relations(&RelationFilter::default(), &page)
        .await
        .unwrap();
    assert!(live.items.is_empty());
    let all = r
        .store
        .relations(
            &RelationFilter {
                include_retracted: true,
                ..RelationFilter::default()
            },
            &page,
        )
        .await
        .unwrap();
    assert_eq!(all.items.len(), 1);
    assert_eq!(all.items[0].status, KnowledgeStatus::Retracted);
}

#[tokio::test]
async fn sweep_marks_stale_relations() {
    let r = rig();
    let rel = r
        .service
        .record_relation(edge(&host(), &host(), FactSource::Discovered), &system())
        .await
        .unwrap();
    r.clock.advance(SignedDuration::from_secs(181 * DAY));
    let report = r.service.sweep().await.unwrap();
    assert_eq!(report.relations_marked_stale, 1);
    let stored = r.store.relation(rel.value.id).await.unwrap().unwrap();
    assert_eq!(stored.value.status, KnowledgeStatus::Stale);
}

#[tokio::test]
async fn renaming_an_entity_checks_names_and_versions() {
    let r = rig();
    let user = UserId::new();
    let a = r.service.create_entity("vpn", "A", user).await.unwrap();
    r.service.create_entity("vpn", "B", user).await.unwrap();
    assert_eq!(
        r.service
            .rename_entity(a.value.id, a.version, "b", user)
            .await
            .unwrap_err(),
        KnowledgeError::DuplicateEntity
    );
    assert_eq!(
        r.service
            .rename_entity(a.value.id, a.version + 3, "C", user)
            .await
            .unwrap_err(),
        KnowledgeError::Conflict
    );
    let renamed = r
        .service
        .rename_entity(a.value.id, a.version, "C", user)
        .await
        .unwrap();
    assert_eq!(renamed.value.name, "C");
    assert_eq!(
        r.service
            .rename_entity(EntityId::new(), 1, "Z", user)
            .await
            .unwrap_err(),
        KnowledgeError::NotFound
    );
}
