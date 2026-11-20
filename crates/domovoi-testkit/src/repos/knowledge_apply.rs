//! The in-memory applier for `StateWrite::Knowledge`, matching the SQLite rules.

use std::collections::BTreeMap;

use domovoi_core::knowledge::{
    Entity, Fact, KnowledgeStatus, KnowledgeWrite, PathConfirmation, Predicate, Relation,
    canonical_json,
};
use domovoi_core::ports::{AggregateRef, CommitError, CommitReceipt, Expectation, Observed};
use domovoi_core::{EntityId, FactRecordId, RelationId};

use super::State;

/// Everything the knowledge store holds, with row versions.
#[derive(Default, Clone)]
pub(super) struct KnowledgeState {
    pub(super) facts: BTreeMap<FactRecordId, (Fact, u64)>,
    pub(super) relations: BTreeMap<RelationId, (Relation, u64)>,
    pub(super) entities: BTreeMap<EntityId, (Entity, u64)>,
    pub(super) confirmations: BTreeMap<FactRecordId, PathConfirmation>,
}

fn conflict(aggregate: AggregateRef, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate,
        expected,
        found,
    }
}

fn constraint(text: &str) -> CommitError {
    CommitError::Constraint(text.to_owned())
}

fn value_text(fact: &Fact) -> Option<String> {
    fact.value.as_ref().map(canonical_json)
}

/// Applies one knowledge write to `state`, updating the receipt's versions.
pub(super) fn apply(
    state: &mut State,
    write: &KnowledgeWrite,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let k = &mut state.knowledge;
    match write {
        KnowledgeWrite::FactInsert(fact) => {
            let aggregate = AggregateRef::fact(fact.id);
            if let Some((_, v)) = k.facts.get(&fact.id) {
                return Err(conflict(aggregate, Expectation::New, Observed::Version(*v)));
            }
            if fact.value.is_none() || !fact.status.is_live() {
                return Err(constraint("a new fact needs a value and a live status"));
            }
            if fact
                .supersedes
                .is_some_and(|old| !k.facts.contains_key(&old))
            {
                return Err(constraint("the superseded fact does not exist"));
            }
            let text = value_text(fact);
            let taken = k.facts.values().any(|(other, _)| {
                other.status.is_live()
                    && other.subject == fact.subject
                    && other.predicate == fact.predicate
                    && value_text(other) == text
            });
            if taken {
                return Err(constraint("a live fact with this value exists"));
            }
            k.facts.insert(fact.id, (fact.clone(), 1));
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        KnowledgeWrite::FactUpdate {
            id,
            expected,
            update,
        } => {
            let aggregate = AggregateRef::fact(*id);
            let Some((fact, v)) = k.facts.get_mut(id) else {
                return Err(conflict(
                    aggregate,
                    Expectation::Version(*expected),
                    Observed::Missing,
                ));
            };
            if *v != *expected {
                return Err(conflict(
                    aggregate,
                    Expectation::Version(*expected),
                    Observed::Version(*v),
                ));
            }
            if !fact.status.is_live() {
                return Err(CommitError::Constraint(format!(
                    "the fact is {}",
                    fact.status.as_str()
                )));
            }
            let retracting = update.status == Some(KnowledgeStatus::Retracted);
            if retracting != update.retraction.is_some() {
                return Err(constraint(
                    "retraction and the retracted status go together",
                ));
            }
            if let Some(status) = update.status {
                fact.status = status;
            }
            if let Some(at) = update.observed_at {
                fact.observed_at = at;
            }
            if let Some(at) = update.last_verified_at {
                fact.last_verified_at = Some(at);
            }
            if let Some((at, by)) = &update.retraction {
                fact.retracted_at = Some(*at);
                fact.retracted_by = Some(by.clone());
            }
            *v += 1;
            receipt.versions.push((aggregate, *v));
            Ok(())
        }
        KnowledgeWrite::RelationInsert(rel) => {
            let aggregate = AggregateRef::relation(rel.id);
            if let Some((_, v)) = k.relations.get(&rel.id) {
                return Err(conflict(aggregate, Expectation::New, Observed::Version(*v)));
            }
            let live = matches!(
                rel.status,
                KnowledgeStatus::Current | KnowledgeStatus::Stale
            );
            if !live || rel.from.is_none() || rel.to.is_none() {
                return Err(constraint(
                    "a new relation needs endpoints and a live status",
                ));
            }
            let taken = k.relations.values().any(|(other, _)| {
                relation_live(other)
                    && other.from == rel.from
                    && other.kind == rel.kind
                    && other.to == rel.to
            });
            if taken {
                return Err(constraint("a live relation with these ends exists"));
            }
            k.relations.insert(rel.id, (rel.clone(), 1));
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        KnowledgeWrite::RelationUpdate {
            id,
            expected,
            update,
        } => {
            let aggregate = AggregateRef::relation(*id);
            let Some((rel, v)) = k.relations.get_mut(id) else {
                return Err(conflict(
                    aggregate,
                    Expectation::Version(*expected),
                    Observed::Missing,
                ));
            };
            if *v != *expected {
                return Err(conflict(
                    aggregate,
                    Expectation::Version(*expected),
                    Observed::Version(*v),
                ));
            }
            if !relation_live(rel) {
                return Err(CommitError::Constraint(format!(
                    "the relation is {}",
                    rel.status.as_str()
                )));
            }
            let retracting = update.status == Some(KnowledgeStatus::Retracted);
            if retracting != update.retraction.is_some() {
                return Err(constraint(
                    "retraction and the retracted status go together",
                ));
            }
            if let Some(status) = update.status {
                rel.status = status;
            }
            if let Some(at) = update.observed_at {
                rel.observed_at = at;
            }
            if let Some((at, by)) = &update.retraction {
                rel.retracted_at = Some(*at);
                rel.retracted_by = Some(by.clone());
            }
            *v += 1;
            receipt.versions.push((aggregate, *v));
            Ok(())
        }
        KnowledgeWrite::EntityInsert(entity) => apply_entity_insert(k, entity, receipt),
        KnowledgeWrite::EntityRename { id, expected, name } => {
            let aggregate = AggregateRef::entity(*id);
            check_entity_version(k, *id, *expected, &aggregate)?;
            let lower = name.to_lowercase();
            if k.entities
                .iter()
                .any(|(other, (e, _))| other != id && e.name.to_lowercase() == lower)
            {
                return Err(constraint("an entity with this name exists"));
            }
            if let Some((entity, v)) = k.entities.get_mut(id) {
                entity.name.clone_from(name);
                *v += 1;
                receipt.versions.push((aggregate, *v));
            }
            Ok(())
        }
        KnowledgeWrite::EntityDelete { id, expected } => {
            let aggregate = AggregateRef::entity(*id);
            check_entity_version(k, *id, *expected, &aggregate)?;
            k.entities.remove(id);
            Ok(())
        }
        KnowledgeWrite::PathConfirmation(c) => {
            let ok = k.facts.get(&c.fact).is_some_and(|(fact, _)| {
                fact.predicate == Predicate::ManagementPath && fact.subject.host() == Some(c.host)
            });
            if !ok {
                return Err(constraint(
                    "a confirmation needs a management path of the same host",
                ));
            }
            k.confirmations.insert(c.fact, c.clone());
            Ok(())
        }
    }
}

/// Whether a relation is live: current or stale.
pub(super) fn relation_live(rel: &Relation) -> bool {
    matches!(
        rel.status,
        KnowledgeStatus::Current | KnowledgeStatus::Stale
    )
}

fn apply_entity_insert(
    k: &mut KnowledgeState,
    entity: &Entity,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::entity(entity.id);
    if let Some((_, v)) = k.entities.get(&entity.id) {
        return Err(conflict(aggregate, Expectation::New, Observed::Version(*v)));
    }
    let lower = entity.name.to_lowercase();
    if k.entities
        .values()
        .any(|(e, _)| e.name.to_lowercase() == lower)
    {
        return Err(constraint("an entity with this name exists"));
    }
    k.entities.insert(entity.id, (entity.clone(), 1));
    receipt.versions.push((aggregate, 1));
    Ok(())
}

fn check_entity_version(
    k: &KnowledgeState,
    id: EntityId,
    expected: u64,
    aggregate: &AggregateRef,
) -> Result<(), CommitError> {
    match k.entities.get(&id) {
        None => Err(conflict(
            aggregate.clone(),
            Expectation::Version(expected),
            Observed::Missing,
        )),
        Some((_, v)) if *v != expected => Err(conflict(
            aggregate.clone(),
            Expectation::Version(expected),
            Observed::Version(*v),
        )),
        Some(_) => Ok(()),
    }
}
