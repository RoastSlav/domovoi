//! The writes the knowledge service hands to the unit of work.

use jiff::Timestamp;

use super::fact::{Fact, KnowledgeStatus};
use super::graph::{Entity, Relation};
use super::paths::PathConfirmation;
use crate::actor::Actor;
use crate::id::{EntityId, FactRecordId, RelationId};

/// One change to the knowledge store.
#[derive(Debug, Clone)]
pub enum KnowledgeWrite {
    /// A new live fact at version 1. The store derives host, search text and liveness.
    FactInsert(Fact),
    /// Changes the mutable columns of a live fact.
    FactUpdate {
        /// The fact to change.
        id: FactRecordId,
        /// The version the caller read.
        expected: u64,
        /// What to change.
        update: FactUpdate,
    },
    /// A new live relation at version 1.
    RelationInsert(Relation),
    /// Changes the mutable columns of a live relation.
    RelationUpdate {
        /// The relation to change.
        id: RelationId,
        /// The version the caller read.
        expected: u64,
        /// What to change.
        update: RelationUpdate,
    },
    /// A new entity at version 1.
    EntityInsert(Entity),
    /// Renames an entity.
    EntityRename {
        /// The entity to rename.
        id: EntityId,
        /// The version the caller read.
        expected: u64,
        /// The new name, already cleaned.
        name: String,
    },
    /// Deletes an entity.
    EntityDelete {
        /// The entity to delete.
        id: EntityId,
        /// The version the caller read.
        expected: u64,
    },
    /// Inserts or replaces the confirmation of one fact.
    PathConfirmation(PathConfirmation),
}

/// The mutable columns of a fact. `None` leaves a column as it is.
#[derive(Debug, Clone, Default)]
pub struct FactUpdate {
    /// The new status.
    pub status: Option<KnowledgeStatus>,
    /// A later observation time.
    pub observed_at: Option<Timestamp>,
    /// A new verification time.
    pub last_verified_at: Option<Timestamp>,
    /// Sets `retracted_at` and `retracted_by`; the status must become `Retracted`.
    pub retraction: Option<(Timestamp, Actor)>,
}

/// The mutable columns of a relation. `None` leaves a column as it is.
#[derive(Debug, Clone, Default)]
pub struct RelationUpdate {
    /// The new status.
    pub status: Option<KnowledgeStatus>,
    /// A later observation time.
    pub observed_at: Option<Timestamp>,
    /// Sets `retracted_at` and `retracted_by`; the status must become `Retracted`.
    pub retraction: Option<(Timestamp, Actor)>,
}
