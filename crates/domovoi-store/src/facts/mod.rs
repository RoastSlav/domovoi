//! The knowledge store on SQLite: applying `KnowledgeWrite`s inside the unit of work and
//! reading facts, relations and entities back.

mod apply_facts;
mod apply_graph;
mod read;
mod rows;

use domovoi_core::knowledge::KnowledgeWrite;
use domovoi_core::ports::{CommitError, CommitReceipt};
use sqlx::SqliteConnection;

/// Applies one knowledge write on the transaction's connection.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &KnowledgeWrite,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match write {
        KnowledgeWrite::FactInsert(fact) => apply_facts::insert(conn, fact, receipt).await,
        KnowledgeWrite::FactUpdate {
            id,
            expected,
            update,
        } => apply_facts::update(conn, *id, *expected, update, receipt).await,
        KnowledgeWrite::PathConfirmation(confirmation) => {
            apply_facts::confirm(conn, confirmation).await
        }
        KnowledgeWrite::RelationInsert(relation) => {
            apply_graph::insert_relation(conn, relation, receipt).await
        }
        KnowledgeWrite::RelationUpdate {
            id,
            expected,
            update,
        } => apply_graph::update_relation(conn, *id, *expected, update, receipt).await,
        KnowledgeWrite::EntityInsert(entity) => {
            apply_graph::insert_entity(conn, entity, receipt).await
        }
        KnowledgeWrite::EntityRename { id, expected, name } => {
            apply_graph::rename_entity(conn, *id, *expected, name, receipt).await
        }
        KnowledgeWrite::EntityDelete { id, expected } => {
            apply_graph::delete_entity(conn, *id, *expected).await
        }
    }
}
