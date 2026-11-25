//! Reading the knowledge store, and the hook that checks management-path coverage.

use async_trait::async_trait;
use jiff::Timestamp;

use super::repos::{Page, Paged, StoreError};
use super::uow::Versioned;
use crate::id::{EntityId, FactRecordId, HostId, RelationId};
use crate::knowledge::{
    CoverageEvidence, CoverageVerdict, Entity, Fact, FactSource, PathConfirmation, Predicate,
    Relation, RelationKind, SubjectRef, UnverifiableReason,
};

/// Which facts to list. Every field narrows the result.
#[derive(Debug, Clone, Default)]
pub struct FactFilter {
    /// Facts whose subject belongs to this host.
    pub host: Option<HostId>,
    /// Facts about exactly this subject.
    pub subject: Option<SubjectRef>,
    /// Facts with this predicate.
    pub predicate: Option<Predicate>,
    /// Facts from this source.
    pub source: Option<FactSource>,
    /// `None` means live statuses only (current, stale, disputed). Superseded and retracted
    /// rows appear only when listed here.
    pub statuses: Option<Vec<crate::knowledge::KnowledgeStatus>>,
}

/// Which relations to list.
#[derive(Debug, Clone, Default)]
pub struct RelationFilter {
    /// Relations with this subject at either end.
    pub subject: Option<SubjectRef>,
    /// Relations of this kind.
    pub kind: Option<RelationKind>,
    /// Whether retracted relations are listed too.
    pub include_retracted: bool,
}

/// A fact found by search, with its rank.
#[derive(Debug, Clone)]
pub struct FactHit {
    /// The fact.
    pub fact: Fact,
    /// The match score; higher is better.
    pub score: f64,
}

/// Read access to facts, relations and entities.
#[async_trait]
pub trait KnowledgeRead: Send + Sync {
    /// One fact with its version.
    async fn fact(&self, id: FactRecordId) -> Result<Option<Versioned<Fact>>, StoreError>;

    /// A page of facts, newest first.
    async fn facts(&self, filter: &FactFilter, page: &Page) -> Result<Paged<Fact>, StoreError>;

    /// Live facts of one subject and predicate, oldest first.
    async fn live_facts(
        &self,
        subject: &SubjectRef,
        predicate: Predicate,
    ) -> Result<Vec<Versioned<Fact>>, StoreError>;

    /// Every live fact, oldest first, at most `limit`. The staleness sweep reads these.
    async fn all_live_facts(&self, limit: u32) -> Result<Vec<Versioned<Fact>>, StoreError>;

    /// Full-text search over live facts only; retracted and superseded facts never match.
    async fn search_facts(
        &self,
        query: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<FactHit>, StoreError>;

    /// The owner's confirmation of a management-path fact, if any.
    async fn confirmation(
        &self,
        fact: FactRecordId,
    ) -> Result<Option<PathConfirmation>, StoreError>;

    /// One relation with its version.
    async fn relation(&self, id: RelationId) -> Result<Option<Versioned<Relation>>, StoreError>;

    /// A page of relations, newest first.
    async fn relations(
        &self,
        filter: &RelationFilter,
        page: &Page,
    ) -> Result<Paged<Relation>, StoreError>;

    /// The live relation for an edge, if any.
    async fn live_relation(
        &self,
        from: &SubjectRef,
        kind: RelationKind,
        to: &SubjectRef,
    ) -> Result<Option<Versioned<Relation>>, StoreError>;

    /// Every live relation, oldest first, at most `limit`.
    async fn all_live_relations(&self, limit: u32) -> Result<Vec<Versioned<Relation>>, StoreError>;

    /// One entity with its version.
    async fn entity(&self, id: EntityId) -> Result<Option<Versioned<Entity>>, StoreError>;

    /// Every entity, by name.
    async fn entities(&self) -> Result<Vec<Versioned<Entity>>, StoreError>;

    /// Whether a live fact or relation names `entity:<id>` as a subject or endpoint, or a live
    /// `depends_on` fact's value is that reference.
    async fn entity_in_use(&self, id: EntityId) -> Result<bool, StoreError>;
}

/// Checks whether a management path is covered by the helper on a host.
#[async_trait]
pub trait ManagementCoverage: Send + Sync {
    /// Whether a coverage check can currently succeed at all.
    fn is_available(&self) -> bool;

    /// Whether the verified helper attestation shows `cidr` (canonical) covered on `host`.
    async fn verdict(&self, host: HostId, cidr: &str) -> CoverageVerdict;

    /// Whether a stored confirmation's `evidence` still holds against the host's current
    /// verified attestation: the identity is equal, or differs only by a reboot while the
    /// attestation is fresh, and the confirmation is not too old. Fails closed by default.
    ///
    /// # Errors
    ///
    /// The reason it cannot be confirmed as current.
    async fn confirmation_current(
        &self,
        _host: HostId,
        _evidence: &CoverageEvidence,
        _confirmed_at: Timestamp,
    ) -> Result<(), UnverifiableReason> {
        Err(UnverifiableReason::NotAvailable)
    }
}
