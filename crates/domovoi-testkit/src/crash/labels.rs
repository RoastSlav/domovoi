//! Stable labels for the writes a crashed commit carried.

use domovoi_core::knowledge::KnowledgeWrite;

/// The label of one knowledge write, for the crash sweep's write list.
pub(super) fn knowledge(write: &KnowledgeWrite) -> String {
    match write {
        KnowledgeWrite::FactInsert(_) => "knowledge:fact_insert",
        KnowledgeWrite::FactUpdate { .. } => "knowledge:fact_update",
        KnowledgeWrite::RelationInsert(_) => "knowledge:relation_insert",
        KnowledgeWrite::RelationUpdate { .. } => "knowledge:relation_update",
        KnowledgeWrite::EntityInsert(_) => "knowledge:entity_insert",
        KnowledgeWrite::EntityRename { .. } => "knowledge:entity_rename",
        KnowledgeWrite::EntityDelete { .. } => "knowledge:entity_delete",
        KnowledgeWrite::PathConfirmation(_) => "knowledge:path_confirmation",
    }
    .to_owned()
}
