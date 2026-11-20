//! The knowledge store's domain types: what the hub knows about hosts and the things on them,
//! with where each piece came from and how fresh it is. Pure data and rules; no I/O.

mod fact;
mod graph;
mod paths;
mod predicate;
mod subject;
mod value;
mod write;

pub use fact::{
    EvidenceError, EvidenceKind, EvidenceRef, Fact, FactSource, KnowledgeStatus,
    source_allows_actor,
};
pub use graph::{
    Entity, RELATION_STALENESS, Relation, RelationKind, clean_entity_name, valid_entity_kind,
};
pub use paths::{
    CoverageEvidence, CoverageVerdict, PathConfirmation, PathState, UnverifiableReason, path_state,
};
pub use predicate::{Cardinality, Predicate, window_end};
pub use subject::{SubjectError, SubjectRef};
pub use value::{ValueError, canonical_json, search_text};
pub use write::{FactUpdate, KnowledgeWrite, RelationUpdate};

#[cfg(test)]
mod kinds_tests {
    use crate::ports::{AggregateKind, JournalKind};

    #[test]
    fn aggregate_and_journal_kinds_round_trip() {
        for kind in [
            AggregateKind::Fact,
            AggregateKind::Relation,
            AggregateKind::Entity,
        ] {
            assert_eq!(AggregateKind::parse(kind.as_str()), Some(kind));
        }
        for kind in [
            JournalKind::FactChange,
            JournalKind::RelationChange,
            JournalKind::EntityChange,
        ] {
            assert_eq!(JournalKind::parse(kind.as_str()), Some(kind));
        }
    }
}
