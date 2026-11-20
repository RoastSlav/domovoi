//! More knowledge-store contracts: confirmations, relations, entities, paging and search.

use domovoi_core::knowledge::{
    KnowledgeStatus, KnowledgeWrite, Predicate, RelationKind, RelationUpdate, SubjectRef,
};
use domovoi_core::ports::{FactFilter, Page, RelationFilter};
use domovoi_core::{HostId, RelationId};
use serde_json::json;

use super::knowledge_store::{
    KnowledgeSubject, commit, entity, fact, is_conflict, is_constraint, owner, relation, retract,
};
use super::{ContractError, ContractResult};
use crate::build::at;

/// One live relation exists per edge; a retracted one frees it and cannot be updated.
pub async fn relation_live_edge_is_unique(s: &dyn KnowledgeSubject) -> ContractResult {
    let a = SubjectRef::Host(HostId::new());
    let b = SubjectRef::Host(HostId::new());
    let r = relation(&a, RelationKind::RunsOn, &b, 10);
    commit(s, KnowledgeWrite::RelationInsert(r.clone())).await?;
    let dup = relation(&a, RelationKind::RunsOn, &b, 11);
    assert!(is_constraint(
        &commit(s, KnowledgeWrite::RelationInsert(dup.clone())).await
    ));
    let found = s
        .knowledge()
        .live_relation(&a, RelationKind::RunsOn, &b)
        .await?
        .ok_or_else(|| ContractError::Setup("missing live".into()))?;
    assert_eq!(found.value.id, r.id);
    assert_eq!(found.version, 1);
    commit(
        s,
        KnowledgeWrite::RelationUpdate {
            id: r.id,
            expected: 1,
            update: RelationUpdate {
                status: Some(KnowledgeStatus::Retracted),
                retraction: Some((at(40), owner())),
                ..RelationUpdate::default()
            },
        },
    )
    .await?;
    let update_again = commit(
        s,
        KnowledgeWrite::RelationUpdate {
            id: r.id,
            expected: 2,
            update: RelationUpdate::default(),
        },
    )
    .await;
    assert!(is_constraint(&update_again));
    commit(s, KnowledgeWrite::RelationInsert(dup)).await?;
    let default = s
        .knowledge()
        .relations(&RelationFilter::default(), &Page::first(10))
        .await?;
    assert_eq!(default.items.len(), 1);
    let all = RelationFilter {
        include_retracted: true,
        ..RelationFilter::default()
    };
    assert_eq!(
        s.knowledge()
            .relations(&all, &Page::first(10))
            .await?
            .items
            .len(),
        2
    );
    Ok(())
}

/// Entity names are unique ignoring case, and rename and delete check the version.
pub async fn entity_names_are_unique_ignoring_case_and_versions_are_checked(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let vpn = entity("VPN");
    commit(s, KnowledgeWrite::EntityInsert(vpn.clone())).await?;
    assert!(is_constraint(
        &commit(s, KnowledgeWrite::EntityInsert(entity("vpn"))).await
    ));
    let pool = entity("Pool");
    commit(s, KnowledgeWrite::EntityInsert(pool.clone())).await?;
    let clash = commit(
        s,
        KnowledgeWrite::EntityRename {
            id: pool.id,
            expected: 1,
            name: "vPn".to_owned(),
        },
    )
    .await;
    assert!(is_constraint(&clash));
    let stale = commit(
        s,
        KnowledgeWrite::EntityRename {
            id: pool.id,
            expected: 5,
            name: "Pool 2".to_owned(),
        },
    )
    .await;
    assert!(is_conflict(&stale));
    commit(
        s,
        KnowledgeWrite::EntityRename {
            id: pool.id,
            expected: 1,
            name: "Pool 2".to_owned(),
        },
    )
    .await?;
    let renamed = s
        .knowledge()
        .entity(pool.id)
        .await?
        .ok_or_else(|| ContractError::Setup("missing entity".into()))?;
    assert_eq!(
        (renamed.value.name.as_str(), renamed.version),
        ("Pool 2", 2)
    );
    let names: Vec<String> = s
        .knowledge()
        .entities()
        .await?
        .into_iter()
        .map(|e| e.value.name)
        .collect();
    assert_eq!(names, vec!["Pool 2".to_owned(), "VPN".to_owned()]);
    let stale_delete = commit(
        s,
        KnowledgeWrite::EntityDelete {
            id: pool.id,
            expected: 1,
        },
    )
    .await;
    assert!(is_conflict(&stale_delete));
    commit(
        s,
        KnowledgeWrite::EntityDelete {
            id: pool.id,
            expected: 2,
        },
    )
    .await?;
    assert!(s.knowledge().entity(pool.id).await?.is_none());
    Ok(())
}

/// Facts page newest first and resume from the cursor.
pub async fn facts_page_orders_newest_first_and_resumes_from_the_cursor(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host_id = HostId::new();
    let host = SubjectRef::Host(host_id);
    let mut ids = Vec::new();
    for i in 0..5 {
        let f = fact(
            &host,
            Predicate::OwnerNote,
            json!(format!("note {i}")),
            10 + i,
        );
        ids.push(f.id);
        commit(s, KnowledgeWrite::FactInsert(f)).await?;
    }
    let filter = FactFilter {
        host: Some(host_id),
        ..FactFilter::default()
    };
    let first = s.knowledge().facts(&filter, &Page::first(2)).await?;
    let got: Vec<_> = first.items.iter().map(|f| f.id).collect();
    assert_eq!(got, vec![ids[4], ids[3]]);
    let cursor = first
        .next_cursor
        .ok_or_else(|| ContractError::Setup("missing more pages".into()))?;
    let second = s
        .knowledge()
        .facts(
            &filter,
            &Page {
                cursor: Some(cursor),
                limit: 2,
            },
        )
        .await?;
    let got: Vec<_> = second.items.iter().map(|f| f.id).collect();
    assert_eq!(got, vec![ids[2], ids[1]]);
    let third = s
        .knowledge()
        .facts(
            &filter,
            &Page {
                cursor: second.next_cursor,
                limit: 2,
            },
        )
        .await?;
    assert_eq!(third.items.len(), 1);
    assert!(third.next_cursor.is_none());
    Ok(())
}

/// Search matches whole words, case-insensitively, in live facts only; a host filter keeps
/// facts without a host.
pub async fn search_matches_whole_words_in_live_facts_only(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host_id = HostId::new();
    let other = HostId::new();
    let a = fact(
        &SubjectRef::Host(host_id),
        Predicate::Role,
        json!("Media Server"),
        10,
    );
    let b = fact(
        &SubjectRef::Host(other),
        Predicate::Role,
        json!("media box"),
        11,
    );
    let ent = entity("Lab");
    let c = fact(
        &SubjectRef::Entity(ent.id),
        Predicate::OwnerNote,
        json!("media archive"),
        12,
    );
    for write in [
        KnowledgeWrite::FactInsert(a.clone()),
        KnowledgeWrite::FactInsert(b.clone()),
        KnowledgeWrite::EntityInsert(ent),
        KnowledgeWrite::FactInsert(c.clone()),
    ] {
        commit(s, write).await?;
    }
    let all = s.knowledge().search_facts("MEDIA", None, 10).await?;
    assert_eq!(all.len(), 3);
    assert!(
        s.knowledge()
            .search_facts("med", None, 10)
            .await?
            .is_empty()
    );
    assert!(s.knowledge().search_facts("", None, 10).await?.is_empty());
    let mut both = s.knowledge().search_facts("media server", None, 10).await?;
    assert_eq!(both.len(), 1);
    assert_eq!(both.remove(0).fact.id, a.id);
    let scoped = s
        .knowledge()
        .search_facts("media", Some(host_id), 10)
        .await?;
    let ids: Vec<_> = scoped.iter().map(|h| h.fact.id).collect();
    assert!(ids.contains(&a.id) && ids.contains(&c.id) && !ids.contains(&b.id));
    let by_predicate = s.knowledge().search_facts("role", None, 10).await?;
    assert_eq!(by_predicate.len(), 2);
    commit(s, retract(a.id, 1)).await?;
    let left = s
        .knowledge()
        .search_facts("media", Some(host_id), 10)
        .await?;
    assert!(left.iter().all(|h| h.fact.id != a.id));
    Ok(())
}

/// An entity is in use while a live fact names it as subject, a live relation names it as an
/// endpoint, or a live `depends_on` value refers to it.
pub async fn entity_in_use_sees_subjects_endpoints_and_depends_on_values(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host = SubjectRef::Host(HostId::new());
    let mut cases = Vec::new();
    for case in 0..3 {
        let e = entity(&format!("e{case}"));
        commit(s, KnowledgeWrite::EntityInsert(e.clone())).await?;
        assert!(!s.knowledge().entity_in_use(e.id).await?);
        let subject = SubjectRef::Entity(e.id);
        match case {
            0 => {
                let f = fact(&subject, Predicate::Role, json!("r"), 10);
                commit(s, KnowledgeWrite::FactInsert(f.clone())).await?;
                cases.push((e.id, Some(retract(f.id, 1)), None::<(RelationId, u64)>));
            }
            1 => {
                let r = relation(&host, RelationKind::RoutesTo, &subject, 10);
                commit(s, KnowledgeWrite::RelationInsert(r.clone())).await?;
                cases.push((e.id, None, Some((r.id, 1))));
            }
            _ => {
                let f = fact(&host, Predicate::DependsOn, json!(subject.to_string()), 10);
                commit(s, KnowledgeWrite::FactInsert(f.clone())).await?;
                cases.push((e.id, Some(retract(f.id, 1)), None));
            }
        }
        assert!(s.knowledge().entity_in_use(e.id).await?);
    }
    for (id, retract_fact, relation) in cases {
        if let Some(write) = retract_fact {
            commit(s, write).await?;
        }
        if let Some((rid, version)) = relation {
            commit(
                s,
                KnowledgeWrite::RelationUpdate {
                    id: rid,
                    expected: version,
                    update: RelationUpdate {
                        status: Some(KnowledgeStatus::Retracted),
                        retraction: Some((at(60), owner())),
                        ..RelationUpdate::default()
                    },
                },
            )
            .await?;
        }
        assert!(!s.knowledge().entity_in_use(id).await?);
    }
    Ok(())
}
