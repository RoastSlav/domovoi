//! Contracts for the knowledge store: facts, relations, entities and confirmations written
//! through the unit of work and read back through `KnowledgeRead`. Run against the in-memory
//! store and SQLite.

use async_trait::async_trait;
use domovoi_core::knowledge::{
    CoverageEvidence, Entity, Fact, FactSource, FactUpdate, KnowledgeStatus, KnowledgeWrite,
    PathConfirmation, Predicate, Relation, RelationKind, SubjectRef,
};
use domovoi_core::ports::{
    AggregateRef, AuditDraft, AuditFilter, AuditLog, CommitError, Expectation, FactFilter,
    JournalDraft, JournalKind, JournalRead, KnowledgeRead, Observed, Page, StateWrite, Transaction,
    UnitOfWork,
};
use domovoi_core::{Actor, EntityId, FactRecordId, HostId, RelationId, UserId};
use serde_json::{Value, json};

use super::{ContractError, ContractResult};
use crate::InMemoryStore;
use crate::build::at;

/// One store the knowledge contracts run against.
#[async_trait]
pub trait KnowledgeSubject: Send + Sync {
    /// The unit of work.
    fn uow(&self) -> &dyn UnitOfWork;
    /// Knowledge reads.
    fn knowledge(&self) -> &dyn KnowledgeRead;
    /// Journal reads.
    fn journal(&self) -> &dyn JournalRead;
    /// The audit log.
    fn audit(&self) -> &dyn AuditLog;
}

impl KnowledgeSubject for InMemoryStore {
    fn uow(&self) -> &dyn UnitOfWork {
        self
    }
    fn knowledge(&self) -> &dyn KnowledgeRead {
        self
    }
    fn journal(&self) -> &dyn JournalRead {
        self
    }
    fn audit(&self) -> &dyn AuditLog {
        self
    }
}

pub(super) fn owner() -> Actor {
    Actor::User(UserId::new())
}

pub(super) fn fact(subject: &SubjectRef, predicate: Predicate, value: Value, secs: i64) -> Fact {
    Fact {
        id: FactRecordId::new(),
        subject: subject.clone(),
        predicate,
        value: Some(value),
        source: FactSource::User,
        status: KnowledgeStatus::Current,
        observed_at: at(secs),
        valid_until: None,
        last_verified_at: None,
        supersedes: None,
        evidence: None,
        actor: owner(),
        created_at: at(secs),
        retracted_at: None,
        retracted_by: None,
        content_purged_at: None,
    }
}

pub(super) fn relation(
    from: &SubjectRef,
    kind: RelationKind,
    to: &SubjectRef,
    secs: i64,
) -> Relation {
    Relation {
        id: RelationId::new(),
        from: Some(from.clone()),
        kind,
        to: Some(to.clone()),
        source: FactSource::User,
        status: KnowledgeStatus::Current,
        observed_at: at(secs),
        evidence: None,
        actor: owner(),
        created_at: at(secs),
        retracted_at: None,
        retracted_by: None,
        content_purged_at: None,
    }
}

pub(super) fn entity(name: &str) -> Entity {
    Entity {
        id: EntityId::new(),
        kind: "vpn".to_owned(),
        name: name.to_owned(),
        created_by: owner(),
        created_at: at(1),
    }
}

pub(super) async fn commit(
    s: &dyn KnowledgeSubject,
    write: KnowledgeWrite,
) -> Result<(), CommitError> {
    s.uow()
        .commit(Transaction::new().write(StateWrite::Knowledge(write)))
        .await
        .map(|_| ())
}

pub(super) fn retract(id: FactRecordId, expected: u64) -> KnowledgeWrite {
    KnowledgeWrite::FactUpdate {
        id,
        expected,
        update: FactUpdate {
            status: Some(KnowledgeStatus::Retracted),
            retraction: Some((at(50), owner())),
            ..FactUpdate::default()
        },
    }
}

pub(super) fn is_conflict(result: &Result<(), CommitError>) -> bool {
    matches!(result, Err(CommitError::Conflict { .. }))
}

pub(super) fn is_constraint(result: &Result<(), CommitError>) -> bool {
    matches!(result, Err(CommitError::Constraint(_)))
}

/// A fact insert and its update advance versions, and a journal row and audit row committed
/// with them land together.
pub async fn fact_insert_and_update_advance_versions_and_journal_together(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host = SubjectRef::Host(HostId::new());
    let f = fact(&host, Predicate::Role, json!("media"), 10);
    let agg = AggregateRef::fact(f.id);
    let draft = JournalDraft {
        aggregate: agg.clone(),
        kind: JournalKind::FactChange,
        payload_v: 1,
        payload: json!({"change": "recorded"}),
        actor: owner(),
        occurred_at: at(10),
    };
    let audit = AuditDraft::new(
        owner(),
        at(10),
        "knowledge.fact.asserted".to_owned(),
        format!("fact:{}", f.id),
        json!({"predicate": "role"}),
    );
    let receipt = s
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Knowledge(KnowledgeWrite::FactInsert(f.clone())))
                .journal(draft)
                .audit(audit),
        )
        .await?;
    assert_eq!(receipt.versions, vec![(agg.clone(), 1)]);
    assert_eq!(receipt.journal_seqs.len(), 1);
    assert_eq!(receipt.audit_seqs.len(), 1);
    let rows = s.journal().entries_for(&agg, 10).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "fact.changed");
    let events = s
        .audit()
        .list(&AuditFilter::default(), &Page::first(50))
        .await?;
    assert!(
        events
            .items
            .iter()
            .any(|e| e.kind == "knowledge.fact.asserted")
    );

    let receipt = s
        .uow()
        .commit(
            Transaction::new().write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
                id: f.id,
                expected: 1,
                update: FactUpdate {
                    last_verified_at: Some(at(20)),
                    ..FactUpdate::default()
                },
            })),
        )
        .await?;
    assert_eq!(receipt.versions, vec![(agg, 2)]);
    let stored = s
        .knowledge()
        .fact(f.id)
        .await?
        .ok_or_else(|| ContractError::Setup("missing stored".into()))?;
    assert_eq!(stored.version, 2);
    assert_eq!(stored.value.last_verified_at, Some(at(20)));
    assert_eq!(stored.value.value, Some(json!("media")));
    Ok(())
}

/// A write that names an old version conflicts and changes nothing, journal included.
pub async fn stale_fact_version_conflicts_and_changes_nothing(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host = SubjectRef::Host(HostId::new());
    let f = fact(&host, Predicate::Role, json!("a"), 10);
    commit(s, KnowledgeWrite::FactInsert(f.clone())).await?;
    let stale = Transaction::new()
        .write(StateWrite::Knowledge(KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 7,
            update: FactUpdate {
                last_verified_at: Some(at(99)),
                ..FactUpdate::default()
            },
        }))
        .journal(JournalDraft {
            aggregate: AggregateRef::fact(f.id),
            kind: JournalKind::FactChange,
            payload_v: 1,
            payload: json!({"change": "verified"}),
            actor: owner(),
            occurred_at: at(99),
        });
    match s.uow().commit(stale).await {
        Err(CommitError::Conflict {
            expected, found, ..
        }) => {
            assert_eq!(expected, Expectation::Version(7));
            assert_eq!(found, Observed::Version(1));
        }
        other => {
            return Err(ContractError::Setup(format!(
                "expected a conflict, got {other:?}"
            )));
        }
    }
    let stored = s
        .knowledge()
        .fact(f.id)
        .await?
        .ok_or_else(|| ContractError::Setup("missing stored".into()))?;
    assert_eq!(stored.version, 1);
    assert_eq!(stored.value.last_verified_at, None);
    assert!(
        s.journal()
            .entries_for(&AggregateRef::fact(f.id), 10)
            .await?
            .is_empty()
    );
    let missing = commit(
        s,
        KnowledgeWrite::FactUpdate {
            id: FactRecordId::new(),
            expected: 1,
            update: FactUpdate::default(),
        },
    )
    .await;
    assert!(is_conflict(&missing));
    let again = commit(s, KnowledgeWrite::FactInsert(f)).await;
    assert!(is_conflict(&again));
    Ok(())
}

/// A retracted fact is not found by search and not listed by default, but is listable by
/// status.
pub async fn retracted_fact_leaves_search_but_stays_listable_by_status(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host_id = HostId::new();
    let host = SubjectRef::Host(host_id);
    let f = fact(&host, Predicate::Role, json!("media server"), 10);
    commit(s, KnowledgeWrite::FactInsert(f.clone())).await?;
    assert_eq!(
        s.knowledge().search_facts("media", None, 10).await?.len(),
        1
    );
    commit(s, retract(f.id, 1)).await?;
    assert!(
        s.knowledge()
            .search_facts("media", None, 10)
            .await?
            .is_empty()
    );
    let filter = FactFilter {
        host: Some(host_id),
        ..FactFilter::default()
    };
    assert!(
        s.knowledge()
            .facts(&filter, &Page::first(10))
            .await?
            .items
            .is_empty()
    );
    assert!(
        s.knowledge()
            .live_facts(&host, Predicate::Role)
            .await?
            .is_empty()
    );
    let retracted = FactFilter {
        statuses: Some(vec![KnowledgeStatus::Retracted]),
        ..filter
    };
    let page = s.knowledge().facts(&retracted, &Page::first(10)).await?;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].retracted_at, Some(at(50)));
    assert_eq!(page.items[0].value, Some(json!("media server")));
    Ok(())
}

/// At most one live row exists per subject, predicate and value; a superseded row frees it.
pub async fn live_value_is_unique_per_subject_and_predicate(
    s: &dyn KnowledgeSubject,
) -> ContractResult {
    let host = SubjectRef::Host(HostId::new());
    let first = fact(&host, Predicate::OwnerNote, json!({"b": 1, "a": 2}), 10);
    commit(s, KnowledgeWrite::FactInsert(first.clone())).await?;
    let twin = fact(&host, Predicate::OwnerNote, json!({"a": 2, "b": 1}), 11);
    assert!(is_constraint(
        &commit(s, KnowledgeWrite::FactInsert(twin.clone())).await
    ));
    let other_predicate = fact(&host, Predicate::Role, json!({"a": 2, "b": 1}), 11);
    commit(s, KnowledgeWrite::FactInsert(other_predicate)).await?;
    commit(
        s,
        KnowledgeWrite::FactUpdate {
            id: first.id,
            expected: 1,
            update: FactUpdate {
                status: Some(KnowledgeStatus::Superseded),
                ..FactUpdate::default()
            },
        },
    )
    .await?;
    let mut again = twin;
    again.supersedes = Some(first.id);
    commit(s, KnowledgeWrite::FactInsert(again)).await?;
    let gone = FactFilter {
        statuses: Some(vec![KnowledgeStatus::Superseded]),
        ..FactFilter::default()
    };
    assert!(
        s.knowledge()
            .facts(&gone, &Page::first(10))
            .await?
            .items
            .iter()
            .any(|f| f.id == first.id)
    );
    Ok(())
}

/// A retracted fact cannot be updated again.
pub async fn retracted_fact_cannot_be_updated(s: &dyn KnowledgeSubject) -> ContractResult {
    let host = SubjectRef::Host(HostId::new());
    let f = fact(&host, Predicate::Role, json!("x"), 10);
    commit(s, KnowledgeWrite::FactInsert(f.clone())).await?;
    commit(s, retract(f.id, 1)).await?;
    let result = commit(
        s,
        KnowledgeWrite::FactUpdate {
            id: f.id,
            expected: 2,
            update: FactUpdate {
                last_verified_at: Some(at(60)),
                ..FactUpdate::default()
            },
        },
    )
    .await;
    assert!(is_constraint(&result));
    let half = commit(
        s,
        KnowledgeWrite::FactUpdate {
            id: FactRecordId::new(),
            expected: 1,
            update: FactUpdate {
                status: Some(KnowledgeStatus::Retracted),
                ..FactUpdate::default()
            },
        },
    )
    .await;
    assert!(half.is_err());
    Ok(())
}

/// A confirmation is inserted, replaced and read back, and is refused for a fact that is not a
/// management path of the same host.
pub async fn confirmation_upserts_and_is_read_back(s: &dyn KnowledgeSubject) -> ContractResult {
    let host = HostId::new();
    let path = fact(
        &SubjectRef::Host(host),
        Predicate::ManagementPath,
        json!({"cidr": "10.0.0.0/8"}),
        10,
    );
    commit(s, KnowledgeWrite::FactInsert(path.clone())).await?;
    let confirmation = |evidence: &str, secs: i64| {
        CoverageEvidence::new(evidence).map(|evidence| PathConfirmation {
            fact: path.id,
            host,
            confirmed_by: owner(),
            confirmed_at: at(secs),
            evidence,
        })
    };
    let confirmation = |evidence: &str, secs: i64| -> Result<PathConfirmation, ContractError> {
        confirmation(evidence, secs).ok_or_else(|| ContractError::Setup("evidence".into()))
    };
    assert!(s.knowledge().confirmation(path.id).await?.is_none());
    commit(
        s,
        KnowledgeWrite::PathConfirmation(confirmation("one", 20)?),
    )
    .await?;
    commit(
        s,
        KnowledgeWrite::PathConfirmation(confirmation("two", 30)?),
    )
    .await?;
    let stored = s
        .knowledge()
        .confirmation(path.id)
        .await?
        .ok_or_else(|| ContractError::Setup("missing stored".into()))?;
    assert_eq!(stored.evidence.as_str(), "two");
    assert_eq!(stored.confirmed_at, at(30));
    let mut wrong_host = confirmation("x", 40)?;
    wrong_host.host = HostId::new();
    assert!(is_constraint(
        &commit(s, KnowledgeWrite::PathConfirmation(wrong_host)).await
    ));
    let role = fact(&SubjectRef::Host(host), Predicate::Role, json!("r"), 11);
    commit(s, KnowledgeWrite::FactInsert(role.clone())).await?;
    let mut not_path = confirmation("x", 40)?;
    not_path.fact = role.id;
    assert!(is_constraint(
        &commit(s, KnowledgeWrite::PathConfirmation(not_path)).await
    ));
    Ok(())
}
