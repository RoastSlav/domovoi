//! Row types and codecs for facts, relations and entities.

use domovoi_core::Actor;
use domovoi_core::knowledge::{
    Entity, Fact, FactSource, KnowledgeStatus, Predicate, Relation, RelationKind,
};
use domovoi_core::ports::{StoreError, Versioned};

use crate::codec::{from_ms, from_ms_opt, parse, uint};
use crate::error::corrupt;

const FACT_COLUMNS: [&str; 17] = [
    "id",
    "subject_ref",
    "predicate",
    "value",
    "source",
    "status",
    "observed_at",
    "valid_until",
    "last_verified_at",
    "supersedes_id",
    "evidence_ref",
    "actor",
    "created_at",
    "retracted_at",
    "retracted_by",
    "content_purged_at",
    "version",
];

const RELATION_COLUMNS: [&str; 14] = [
    "id",
    "from_ref",
    "to_ref",
    "kind",
    "source",
    "status",
    "observed_at",
    "evidence_ref",
    "actor",
    "created_at",
    "retracted_at",
    "retracted_by",
    "content_purged_at",
    "version",
];

fn prefixed(columns: &[&str], p: &str) -> String {
    columns
        .iter()
        .map(|c| format!("{p}{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The fact columns in `FactRow` order, with a table prefix such as `"f."` or `""`.
pub(super) fn fact_columns(p: &str) -> String {
    prefixed(&FACT_COLUMNS, p)
}

/// The relation columns in `RelationRow` order.
pub(super) fn relation_columns(p: &str) -> String {
    prefixed(&RELATION_COLUMNS, p)
}

fn opt<T, E: std::fmt::Display>(text: Option<&str>, what: &str) -> Result<Option<T>, StoreError>
where
    T: std::str::FromStr<Err = E>,
{
    text.map(|t| parse::<T>(t, what)).transpose()
}

/// One `facts` row.
#[derive(sqlx::FromRow)]
pub(super) struct FactRow {
    id: String,
    subject_ref: String,
    predicate: String,
    value: Option<String>,
    source: String,
    status: String,
    observed_at: i64,
    valid_until: Option<i64>,
    last_verified_at: Option<i64>,
    supersedes_id: Option<String>,
    evidence_ref: Option<String>,
    actor: String,
    created_at: i64,
    retracted_at: Option<i64>,
    retracted_by: Option<String>,
    content_purged_at: Option<i64>,
    version: i64,
}

/// A fact row with its search score.
#[derive(sqlx::FromRow)]
pub(super) struct HitRow {
    #[sqlx(flatten)]
    pub fact: FactRow,
    pub score: f64,
}

impl FactRow {
    /// The `created_at` and id, for paging.
    pub fn key(&self) -> (i64, String) {
        (self.created_at, self.id.clone())
    }

    pub fn into_fact(self) -> Result<Versioned<Fact>, StoreError> {
        let predicate = Predicate::parse(&self.predicate)
            .ok_or_else(|| corrupt("fact predicate", &self.predicate))?;
        let value = self
            .value
            .as_deref()
            .map(|t| crate::codec::from_json(t, "fact value"))
            .transpose()?;
        let fact = Fact {
            id: parse(&self.id, "fact id")?,
            subject: parse(&self.subject_ref, "fact subject")?,
            predicate,
            value,
            source: FactSource::parse(&self.source)
                .ok_or_else(|| corrupt("fact source", &self.source))?,
            status: KnowledgeStatus::parse(&self.status)
                .ok_or_else(|| corrupt("fact status", &self.status))?,
            observed_at: from_ms(self.observed_at)?,
            valid_until: from_ms_opt(self.valid_until)?,
            last_verified_at: from_ms_opt(self.last_verified_at)?,
            supersedes: opt(self.supersedes_id.as_deref(), "fact supersedes")?,
            evidence: opt(self.evidence_ref.as_deref(), "fact evidence")?,
            actor: parse::<Actor>(&self.actor, "fact actor")?,
            created_at: from_ms(self.created_at)?,
            retracted_at: from_ms_opt(self.retracted_at)?,
            retracted_by: opt::<Actor, _>(self.retracted_by.as_deref(), "fact retracted_by")?,
            content_purged_at: from_ms_opt(self.content_purged_at)?,
        };
        Ok(Versioned {
            value: fact,
            version: uint(self.version),
        })
    }
}

/// One `relations` row.
#[derive(sqlx::FromRow)]
pub(super) struct RelationRow {
    id: String,
    from_ref: Option<String>,
    to_ref: Option<String>,
    kind: String,
    source: String,
    status: String,
    observed_at: i64,
    evidence_ref: Option<String>,
    actor: String,
    created_at: i64,
    retracted_at: Option<i64>,
    retracted_by: Option<String>,
    content_purged_at: Option<i64>,
    version: i64,
}

impl RelationRow {
    pub fn key(&self) -> (i64, String) {
        (self.created_at, self.id.clone())
    }

    pub fn into_relation(self) -> Result<Versioned<Relation>, StoreError> {
        let relation = Relation {
            id: parse(&self.id, "relation id")?,
            from: opt(self.from_ref.as_deref(), "relation from")?,
            kind: RelationKind::parse(&self.kind)
                .ok_or_else(|| corrupt("relation kind", &self.kind))?,
            to: opt(self.to_ref.as_deref(), "relation to")?,
            source: FactSource::parse(&self.source)
                .ok_or_else(|| corrupt("relation source", &self.source))?,
            status: KnowledgeStatus::parse(&self.status)
                .ok_or_else(|| corrupt("relation status", &self.status))?,
            observed_at: from_ms(self.observed_at)?,
            evidence: opt(self.evidence_ref.as_deref(), "relation evidence")?,
            actor: parse::<Actor>(&self.actor, "relation actor")?,
            created_at: from_ms(self.created_at)?,
            retracted_at: from_ms_opt(self.retracted_at)?,
            retracted_by: opt::<Actor, _>(self.retracted_by.as_deref(), "relation retracted_by")?,
            content_purged_at: from_ms_opt(self.content_purged_at)?,
        };
        Ok(Versioned {
            value: relation,
            version: uint(self.version),
        })
    }
}

/// One `entities` row.
#[derive(sqlx::FromRow)]
pub(super) struct EntityRow {
    id: String,
    kind: String,
    name: String,
    created_by: String,
    created_at: i64,
    version: i64,
}

impl EntityRow {
    pub fn into_entity(self) -> Result<Versioned<Entity>, StoreError> {
        let entity = Entity {
            id: parse(&self.id, "entity id")?,
            kind: self.kind,
            name: self.name,
            created_by: parse::<Actor>(&self.created_by, "entity created_by")?,
            created_at: from_ms(self.created_at)?,
        };
        Ok(Versioned {
            value: entity,
            version: uint(self.version),
        })
    }
}
