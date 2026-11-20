//! What the knowledge API takes and returns.

use domovoi_core::knowledge::{Entity, Fact, KnowledgeStatus, PathConfirmation, Relation};
use domovoi_core::ports::Versioned;
use domovoi_engine::knowledge::ManagementPathView;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Facts to assert. The source is always the signed-in owner; a `source` or `actor` field is
/// refused.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeFactInput {
    /// The subject: `host:<id>`, `container:<host id>/<name>`, `unit:<host id>/<name>` or
    /// `entity:<id>`.
    pub subject: String,
    /// The predicate, for example `role`.
    pub predicate: String,
    /// The value; its shape depends on the predicate.
    #[schema(value_type = serde_json::Value)]
    pub value: serde_json::Value,
    /// What it was derived from, as `<kind>:<id>`.
    #[serde(default)]
    pub evidence: Option<String>,
    /// After this time the fact no longer holds.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub valid_until: Option<Timestamp>,
    /// The password, needed for safety-critical predicates.
    #[serde(default)]
    pub password: Option<String>,
}

/// An action on one fact, at the version the client read.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeFactAction {
    /// The version the client read.
    pub expected_version: u64,
    /// The password, needed to verify or accept a safety-critical fact and to confirm a path.
    #[serde(default)]
    pub password: Option<String>,
}

/// One fact.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct KnowledgeFactDto {
    /// Fact id.
    pub id: String,
    /// The subject reference.
    pub subject: String,
    /// The predicate.
    pub predicate: String,
    /// The value; `null` once the retention purge cleared it.
    #[schema(value_type = Option<serde_json::Value>)]
    pub value: Option<serde_json::Value>,
    /// `user`, `discovered`, `integration`, `inferred` or `imported`.
    pub source: String,
    /// The status now: `current`, `stale`, `disputed`, `superseded` or `retracted`.
    pub status: String,
    /// The status stored in the row.
    pub stored_status: String,
    /// The version a later change must name.
    pub version: u64,
    /// When it was last seen to hold.
    #[schema(value_type = String)]
    pub observed_at: Timestamp,
    /// After this time it no longer holds.
    #[schema(value_type = Option<String>)]
    pub valid_until: Option<Timestamp>,
    /// When it was last confirmed.
    #[schema(value_type = Option<String>)]
    pub last_verified_at: Option<Timestamp>,
    /// When it stops being fresh.
    #[schema(value_type = Option<String>)]
    pub fresh_until: Option<Timestamp>,
    /// The fact it replaced.
    pub supersedes: Option<String>,
    /// What it was derived from.
    pub evidence: Option<String>,
    /// Who wrote it.
    pub actor: String,
    /// When it was written.
    #[schema(value_type = String)]
    pub created_at: Timestamp,
    /// When it was retracted.
    #[schema(value_type = Option<String>)]
    pub retracted_at: Option<Timestamp>,
    /// Whether the retention purge cleared its content.
    pub content_purged: bool,
}

impl KnowledgeFactDto {
    /// The wire form of a stored fact at `now`.
    #[must_use]
    pub fn new(fact: &Versioned<Fact>, now: Timestamp) -> Self {
        let f = &fact.value;
        Self {
            id: f.id.to_string(),
            subject: f.subject.to_string(),
            predicate: f.predicate.as_str().to_owned(),
            value: f.value.clone(),
            source: f.source.as_str().to_owned(),
            status: f.effective_status(now).as_str().to_owned(),
            stored_status: f.status.as_str().to_owned(),
            version: fact.version,
            observed_at: f.observed_at,
            valid_until: f.valid_until,
            last_verified_at: f.last_verified_at,
            fresh_until: f.fresh_until(),
            supersedes: f.supersedes.map(|id| id.to_string()),
            evidence: f.evidence.as_ref().map(ToString::to_string),
            actor: f.actor.to_string(),
            created_at: f.created_at,
            retracted_at: f.retracted_at,
            content_purged: f.content_purged_at.is_some(),
        }
    }
}

/// A page of facts.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct KnowledgeFactPageDto {
    /// The facts, newest first.
    pub items: Vec<KnowledgeFactDto>,
    /// Pass as `cursor` for the next page; `null` on the last.
    pub next_cursor: Option<String>,
}

/// Which facts to list.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FactListQuery {
    /// Host name.
    pub host: Option<String>,
    /// A subject reference.
    pub subject: Option<String>,
    /// A predicate.
    pub predicate: Option<String>,
    /// A source.
    pub source: Option<String>,
    /// Comma-separated statuses; default is every live status.
    pub status: Option<String>,
    /// Full-text search; ignores `cursor` and sets no `next_cursor`.
    pub q: Option<String>,
    /// Page size, default 50, at most 200 (50 with `q`).
    pub limit: Option<u16>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

/// The confirmation of a management path. The helper's evidence is not returned.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PathConfirmationDto {
    /// Who confirmed it.
    pub confirmed_by: String,
    /// When.
    #[schema(value_type = String)]
    pub confirmed_at: Timestamp,
}

impl From<&PathConfirmation> for PathConfirmationDto {
    fn from(c: &PathConfirmation) -> Self {
        Self {
            confirmed_by: c.confirmed_by.to_string(),
            confirmed_at: c.confirmed_at,
        }
    }
}

/// One management path.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManagementPathDto {
    /// The path fact.
    pub fact: KnowledgeFactDto,
    /// `candidate`, `unconfirmed`, `confirmed` or `stale`.
    pub state: String,
    /// The owner's confirmation, if there is one.
    pub confirmation: Option<PathConfirmationDto>,
}

impl ManagementPathDto {
    /// The wire form of a path at `now`.
    #[must_use]
    pub fn new(view: &ManagementPathView, now: Timestamp) -> Self {
        Self {
            fact: KnowledgeFactDto::new(&view.fact, now),
            state: view.state.as_str().to_owned(),
            confirmation: view.confirmation.as_ref().map(PathConfirmationDto::from),
        }
    }
}

/// A host's management paths.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ManagementPathsDto {
    /// The host name.
    pub host: String,
    /// Whether the hub can currently verify the helper's coverage; paths cannot be confirmed
    /// while it cannot.
    pub coverage_available: bool,
    /// The paths, by range.
    pub paths: Vec<ManagementPathDto>,
}

/// A relation to assert. The source is always the signed-in owner.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationInput {
    /// The source end, a subject reference.
    pub from: String,
    /// `depends_on`, `runs_on`, `routes_to`, `stores_on`, `backs_up_to` or `managed_from`.
    pub kind: String,
    /// The target end, a subject reference.
    pub to: String,
    /// What it was derived from, as `<kind>:<id>`.
    #[serde(default)]
    pub evidence: Option<String>,
}

/// Retracting a relation.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationRetractInput {
    /// The version the client read.
    pub expected_version: u64,
}

/// One relation.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelationDto {
    /// Relation id.
    pub id: String,
    /// The source end; `null` once the retention purge cleared it.
    pub from: Option<String>,
    /// What the edge means.
    pub kind: String,
    /// The target end; `null` once the retention purge cleared it.
    pub to: Option<String>,
    /// Where it came from.
    pub source: String,
    /// The status now.
    pub status: String,
    /// The status stored in the row.
    pub stored_status: String,
    /// The version a later change must name.
    pub version: u64,
    /// When it was last seen to hold.
    #[schema(value_type = String)]
    pub observed_at: Timestamp,
    /// What it was derived from.
    pub evidence: Option<String>,
    /// Who wrote it.
    pub actor: String,
    /// When it was written.
    #[schema(value_type = String)]
    pub created_at: Timestamp,
    /// When it was retracted.
    #[schema(value_type = Option<String>)]
    pub retracted_at: Option<Timestamp>,
    /// Whether the retention purge cleared its endpoints.
    pub content_purged: bool,
}

impl RelationDto {
    /// The wire form of a stored relation at `now`.
    #[must_use]
    pub fn new(relation: &Versioned<Relation>, now: Timestamp) -> Self {
        let r = &relation.value;
        Self {
            id: r.id.to_string(),
            from: r.from.as_ref().map(ToString::to_string),
            kind: r.kind.as_str().to_owned(),
            to: r.to.as_ref().map(ToString::to_string),
            source: r.source.as_str().to_owned(),
            status: r.effective_status(now).as_str().to_owned(),
            stored_status: r.status.as_str().to_owned(),
            version: relation.version,
            observed_at: r.observed_at,
            evidence: r.evidence.as_ref().map(ToString::to_string),
            actor: r.actor.to_string(),
            created_at: r.created_at,
            retracted_at: r.retracted_at,
            content_purged: r.content_purged_at.is_some(),
        }
    }
}

/// A page of relations.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelationPageDto {
    /// The relations, newest first.
    pub items: Vec<RelationDto>,
    /// Pass as `cursor` for the next page; `null` on the last.
    pub next_cursor: Option<String>,
}

/// Which relations to list.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RelationListQuery {
    /// A subject reference matched at either end.
    pub subject: Option<String>,
    /// A relation kind.
    pub kind: Option<String>,
    /// Whether retracted relations are listed too.
    pub include_retracted: Option<bool>,
    /// Page size, default 50, at most 200.
    pub limit: Option<u16>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

/// One entity.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EntityDto {
    /// Entity id.
    pub id: String,
    /// The kind, a lowercase slug.
    pub kind: String,
    /// The name.
    pub name: String,
    /// The owner who created it.
    pub created_by: String,
    /// When it was created.
    #[schema(value_type = String)]
    pub created_at: Timestamp,
    /// The version a later change must name.
    pub version: u64,
}

impl From<&Versioned<Entity>> for EntityDto {
    fn from(e: &Versioned<Entity>) -> Self {
        Self {
            id: e.value.id.to_string(),
            kind: e.value.kind.clone(),
            name: e.value.name.clone(),
            created_by: e.value.created_by.to_string(),
            created_at: e.value.created_at,
            version: e.version,
        }
    }
}

/// Creating an entity.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityCreate {
    /// A lowercase slug of 1 to 32 characters.
    pub kind: String,
    /// A name of 1 to 80 characters, unique ignoring case.
    pub name: String,
}

/// Renaming an entity.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityRename {
    /// The new name.
    pub name: String,
    /// The version the client read.
    pub expected_version: u64,
}

/// The version a delete names.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EntityDeleteQuery {
    /// The version the client read.
    pub expected_version: u64,
}

/// A status list from a query string: `None` when any word is not a status.
#[must_use]
pub fn parse_statuses(text: &str) -> Option<Vec<KnowledgeStatus>> {
    text.split(',')
        .map(|part| KnowledgeStatus::parse(part.trim()))
        .collect()
}
