//! Aggregate references for the knowledge store.

use super::{AggregateKind, AggregateRef};

impl AggregateRef {
    /// A knowledge fact reference.
    #[must_use]
    pub fn fact(id: crate::id::FactRecordId) -> Self {
        Self {
            kind: AggregateKind::Fact,
            id: id.to_string(),
        }
    }

    /// A knowledge relation reference.
    #[must_use]
    pub fn relation(id: crate::id::RelationId) -> Self {
        Self {
            kind: AggregateKind::Relation,
            id: id.to_string(),
        }
    }

    /// A knowledge entity reference.
    #[must_use]
    pub fn entity(id: crate::id::EntityId) -> Self {
        Self {
            kind: AggregateKind::Entity,
            id: id.to_string(),
        }
    }
}
