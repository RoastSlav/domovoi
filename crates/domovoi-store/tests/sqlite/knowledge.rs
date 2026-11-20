//! The knowledge store on SQLite: the shared contract suite, and what SQLite stores beyond it.

use async_trait::async_trait;
use domovoi_core::knowledge::{
    CoverageEvidence, Fact, FactSource, FactUpdate, KnowledgeStatus, KnowledgeWrite,
    PathConfirmation, Predicate, Relation, RelationKind, RelationUpdate, SubjectRef,
};
use domovoi_core::ports::{
    AuditLog, CommitError, FactFilter, JournalRead, KnowledgeRead, Page, StateWrite, Transaction,
    UnitOfWork,
};
use domovoi_core::{Actor, FactRecordId, HostId, RelationId, UserId};
use domovoi_store::{RetentionPolicy, Store};
use domovoi_testkit::build::at;
use domovoi_testkit::contracts::KnowledgeSubject;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::common;

struct SqliteKnowledge(Store);

#[async_trait]
impl KnowledgeSubject for SqliteKnowledge {
    fn uow(&self) -> &dyn UnitOfWork {
        &self.0
    }

    fn knowledge(&self) -> &dyn KnowledgeRead {
        &self.0
    }

    fn journal(&self) -> &dyn JournalRead {
        &self.0
    }

    fn audit(&self) -> &dyn AuditLog {
        &self.0
    }
}

macro_rules! knowledge_contract_tests {
    ($($name:ident => $suite:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                let fresh = common::Fresh::new().await;
                let subject = SqliteKnowledge(fresh.store.clone());
                domovoi_testkit::contracts::$suite(&subject).await.unwrap();
            }
        )*
    };
}

knowledge_contract_tests! {
    sqlite_fact_insert_and_update_advance_versions_and_journal_together
        => fact_insert_and_update_advance_versions_and_journal_together,
    sqlite_stale_fact_version_conflicts_and_changes_nothing
        => stale_fact_version_conflicts_and_changes_nothing,
    sqlite_retracted_fact_leaves_search_but_stays_listable_by_status
        => retracted_fact_leaves_search_but_stays_listable_by_status,
    sqlite_live_value_is_unique_per_subject_and_predicate
        => live_value_is_unique_per_subject_and_predicate,
    sqlite_retracted_fact_cannot_be_updated => retracted_fact_cannot_be_updated,
    sqlite_confirmation_upserts_and_is_read_back => confirmation_upserts_and_is_read_back,
    sqlite_relation_live_edge_is_unique => relation_live_edge_is_unique,
    sqlite_entity_names_are_unique_ignoring_case_and_versions_are_checked
        => entity_names_are_unique_ignoring_case_and_versions_are_checked,
    sqlite_facts_page_orders_newest_first_and_resumes_from_the_cursor
        => facts_page_orders_newest_first_and_resumes_from_the_cursor,
    sqlite_search_matches_whole_words_in_live_facts_only
        => search_matches_whole_words_in_live_facts_only,
    sqlite_entity_in_use_sees_subjects_endpoints_and_depends_on_values
        => entity_in_use_sees_subjects_endpoints_and_depends_on_values,
}

const DAY: i64 = 86_400;

fn owner() -> Actor {
    Actor::User(UserId::new())
}

fn fact(subject: &SubjectRef, predicate: Predicate, value: Value) -> Fact {
    Fact {
        id: FactRecordId::new(),
        subject: subject.clone(),
        predicate,
        value: Some(value),
        source: FactSource::User,
        status: KnowledgeStatus::Current,
        observed_at: at(10),
        valid_until: None,
        last_verified_at: None,
        supersedes: None,
        evidence: None,
        actor: owner(),
        created_at: at(10),
        retracted_at: None,
        retracted_by: None,
        content_purged_at: None,
    }
}

fn relation(from: &SubjectRef, to: &SubjectRef) -> Relation {
    Relation {
        id: RelationId::new(),
        from: Some(from.clone()),
        kind: RelationKind::DependsOn,
        to: Some(to.clone()),
        source: FactSource::User,
        status: KnowledgeStatus::Current,
        observed_at: at(10),
        evidence: None,
        actor: owner(),
        created_at: at(10),
        retracted_at: None,
        retracted_by: None,
        content_purged_at: None,
    }
}

async fn commit(store: &Store, write: KnowledgeWrite) -> Result<(), CommitError> {
    store
        .commit(Transaction::new().write(StateWrite::Knowledge(write)))
        .await
        .map(|_| ())
}

fn retract_at(secs: i64) -> FactUpdate {
    FactUpdate {
        status: Some(KnowledgeStatus::Retracted),
        retraction: Some((at(secs), owner())),
        ..FactUpdate::default()
    }
}

fn host() -> SubjectRef {
    SubjectRef::Host(HostId::new())
}

async fn count(store: &Store, sql: &'static str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql)
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    n
}

#[tokio::test]
async fn retracted_fact_leaves_the_full_text_index_in_the_same_transaction() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let subject = host();
    let f = fact(&subject, Predicate::Role, json!("media server"));
    commit(store, KnowledgeWrite::FactInsert(f.clone()))
        .await
        .unwrap();
    assert_eq!(
        store.search_facts("media", None, 10).await.unwrap().len(),
        1
    );

    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 1,
            update: retract_at(20),
        },
    )
    .await
    .unwrap();

    let (hits,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM facts_fts WHERE facts_fts MATCH 'media'")
            .fetch_one(store.pool_for_tests())
            .await
            .unwrap();
    assert_eq!(hits, 0);
    assert!(
        store
            .search_facts("media", None, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let filter = FactFilter {
        statuses: Some(vec![KnowledgeStatus::Retracted]),
        ..FactFilter::default()
    };
    let page = store.facts(&filter, &Page::first(10)).await.unwrap();
    assert_eq!(page.items.len(), 1);
}

#[tokio::test]
async fn purge_clears_retracted_content_after_the_configured_days_and_keeps_the_tombstone() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let subject = host();
    let evidence = "note:0190a0a0-0000-7000-8000-000000000001".parse().unwrap();
    let mut f = fact(&subject, Predicate::Role, json!("media server"));
    f.evidence = Some(evidence);
    let older = fact(&subject, Predicate::Role, json!("old"));
    commit(store, KnowledgeWrite::FactInsert(older.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: older.id,
            expected: 1,
            update: FactUpdate {
                status: Some(KnowledgeStatus::Superseded),
                ..FactUpdate::default()
            },
        },
    )
    .await
    .unwrap();
    f.supersedes = Some(older.id);
    commit(store, KnowledgeWrite::FactInsert(f.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 1,
            update: retract_at(0),
        },
    )
    .await
    .unwrap();
    let to = host();
    let r = relation(&subject, &to);
    commit(store, KnowledgeWrite::RelationInsert(r.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::RelationUpdate {
            id: r.id,
            expected: 1,
            update: RelationUpdate {
                status: Some(KnowledgeStatus::Retracted),
                retraction: Some((at(0), owner())),
                ..RelationUpdate::default()
            },
        },
    )
    .await
    .unwrap();

    let policy = RetentionPolicy {
        retracted_facts: SignedDuration::from_hours(24 * 30),
        ..RetentionPolicy::default()
    };
    store.purge(at(29 * DAY), &policy).await.unwrap();
    let kept = store.fact(f.id).await.unwrap().unwrap().value;
    assert!(kept.value.is_some());

    let report = store.purge(at(31 * DAY), &policy).await.unwrap();
    assert!(report.retracted_facts >= 2);
    let purged = store.fact(f.id).await.unwrap().unwrap().value;
    assert_eq!(purged.value, None);
    assert!(purged.content_purged_at.is_some());
    assert_eq!(purged.id, f.id);
    assert_eq!(purged.subject, subject);
    assert_eq!(purged.predicate, Predicate::Role);
    assert_eq!(purged.source, FactSource::User);
    assert_eq!(purged.status, KnowledgeStatus::Retracted);
    assert_eq!(purged.actor, f.actor);
    assert_eq!(purged.supersedes, Some(older.id));
    assert_eq!(purged.evidence, f.evidence);
    let (text,): (Option<String>,) = sqlx::query_as("SELECT search_text FROM facts WHERE id = ?")
        .bind(f.id.to_string())
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(text, None);

    let gone = store.relation(r.id).await.unwrap().unwrap().value;
    assert_eq!((gone.from, gone.to), (None, None));
    assert!(gone.content_purged_at.is_some());
}

#[tokio::test]
async fn purge_does_not_touch_live_or_superseded_facts() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let subject = host();
    let current = fact(&subject, Predicate::Role, json!("a"));
    let old = fact(&subject, Predicate::Criticality, json!("warning"));
    commit(store, KnowledgeWrite::FactInsert(current.clone()))
        .await
        .unwrap();
    commit(store, KnowledgeWrite::FactInsert(old.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: old.id,
            expected: 1,
            update: FactUpdate {
                status: Some(KnowledgeStatus::Superseded),
                ..FactUpdate::default()
            },
        },
    )
    .await
    .unwrap();
    let report = store
        .purge(at(3000 * DAY), &RetentionPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.retracted_facts, 0);
    for id in [current.id, old.id] {
        assert!(store.fact(id).await.unwrap().unwrap().value.value.is_some());
    }
}

#[tokio::test]
async fn purged_fact_cannot_be_found_by_search_or_listed_with_a_value() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let f = fact(&host(), Predicate::Role, json!("media server"));
    commit(store, KnowledgeWrite::FactInsert(f.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 1,
            update: retract_at(0),
        },
    )
    .await
    .unwrap();
    store
        .purge(at(40 * DAY), &RetentionPolicy::default())
        .await
        .unwrap();
    assert!(
        store
            .search_facts("media", None, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let filter = FactFilter {
        statuses: Some(vec![KnowledgeStatus::Retracted]),
        ..FactFilter::default()
    };
    let page = store.facts(&filter, &Page::first(10)).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].value, None);
}

#[tokio::test]
async fn unique_live_value_index_refuses_a_second_live_row_but_not_a_superseded_one() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let subject = host();
    let first = fact(&subject, Predicate::OwnerNote, json!("same"));
    commit(store, KnowledgeWrite::FactInsert(first.clone()))
        .await
        .unwrap();
    let second = fact(&subject, Predicate::OwnerNote, json!("same"));
    let refused = commit(store, KnowledgeWrite::FactInsert(second.clone())).await;
    assert!(
        matches!(refused, Err(CommitError::Constraint(_))),
        "{refused:?}"
    );
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: first.id,
            expected: 1,
            update: FactUpdate {
                status: Some(KnowledgeStatus::Superseded),
                ..FactUpdate::default()
            },
        },
    )
    .await
    .unwrap();
    commit(store, KnowledgeWrite::FactInsert(second))
        .await
        .unwrap();
}

#[tokio::test]
async fn fact_update_of_a_retracted_fact_is_refused() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let f = fact(&host(), Predicate::Role, json!("x"));
    commit(store, KnowledgeWrite::FactInsert(f.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 1,
            update: retract_at(5),
        },
    )
    .await
    .unwrap();
    let again = commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 2,
            update: FactUpdate {
                last_verified_at: Some(at(9)),
                ..FactUpdate::default()
            },
        },
    )
    .await;
    assert!(
        matches!(again, Err(CommitError::Constraint(_))),
        "{again:?}"
    );
}

#[tokio::test]
async fn confirmation_for_another_host_or_a_non_path_fact_is_refused() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let host_id = HostId::new();
    let subject = SubjectRef::Host(host_id);
    let path = fact(
        &subject,
        Predicate::ManagementPath,
        json!({"cidr": "10.0.0.0/8"}),
    );
    let role = fact(&subject, Predicate::Role, json!("x"));
    commit(store, KnowledgeWrite::FactInsert(path.clone()))
        .await
        .unwrap();
    commit(store, KnowledgeWrite::FactInsert(role.clone()))
        .await
        .unwrap();
    let confirmation = |fact: FactRecordId, host: HostId| PathConfirmation {
        fact,
        host,
        confirmed_by: owner(),
        confirmed_at: at(20),
        evidence: CoverageEvidence::new("e").unwrap(),
    };
    let other_host = commit(
        store,
        KnowledgeWrite::PathConfirmation(confirmation(path.id, HostId::new())),
    )
    .await;
    assert!(matches!(other_host, Err(CommitError::Constraint(_))));
    let non_path = commit(
        store,
        KnowledgeWrite::PathConfirmation(confirmation(role.id, host_id)),
    )
    .await;
    assert!(matches!(non_path, Err(CommitError::Constraint(_))));
    commit(
        store,
        KnowledgeWrite::PathConfirmation(confirmation(path.id, host_id)),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn knowledge_writes_roll_back_with_a_failing_later_write() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let good = fact(&host(), Predicate::Role, json!("x"));
    let bad = fact(&host(), Predicate::Role, json!("y"));
    let missing = KnowledgeWrite::FactUpdate {
        id: bad.id,
        expected: 1,
        update: FactUpdate::default(),
    };
    let result = store
        .commit(
            Transaction::new()
                .write(StateWrite::Knowledge(KnowledgeWrite::FactInsert(
                    good.clone(),
                )))
                .write(StateWrite::Knowledge(missing)),
        )
        .await;
    assert!(result.is_err());
    assert!(store.fact(good.id).await.unwrap().is_none());
    assert_eq!(count(store, "SELECT count(*) FROM journal").await, 0);
}

#[tokio::test]
async fn retracted_facts_days_policy_field_is_applied() {
    let fresh = common::Fresh::new().await;
    let store = &fresh.store;
    let f = fact(&host(), Predicate::Role, json!("x"));
    commit(store, KnowledgeWrite::FactInsert(f.clone()))
        .await
        .unwrap();
    commit(
        store,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 1,
            update: retract_at(0),
        },
    )
    .await
    .unwrap();
    let short = RetentionPolicy {
        retracted_facts: SignedDuration::from_hours(24 * 2),
        ..RetentionPolicy::default()
    };
    let report = store.purge(at(3 * DAY), &short).await.unwrap();
    assert_eq!(report.retracted_facts, 1);
}
