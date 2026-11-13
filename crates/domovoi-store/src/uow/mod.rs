//! The SQLite unit of work: one `BEGIN IMMEDIATE ... COMMIT` on the single writer connection.

mod effects;
mod journal;
mod reads;
mod writes;

use async_trait::async_trait;
use domovoi_core::ports::{
    CommitError, CommitReceipt, JournalDraft, StoreError, Transaction, UnitOfWork,
};

use crate::db::Store;
use crate::repos::insert_audit;

/// Maps a store failure onto the unit-of-work error.
fn map_store(err: StoreError) -> CommitError {
    match err {
        StoreError::Conflict(message) => CommitError::Constraint(message),
        StoreError::NotFound => CommitError::Constraint("the record does not exist".to_owned()),
        StoreError::Backend(message) => CommitError::Backend(message),
    }
}

/// Maps a SQLite failure onto the unit-of-work error.
fn backend(err: sqlx::Error) -> CommitError {
    match &err {
        sqlx::Error::Database(e) if e.is_unique_violation() || e.is_foreign_key_violation() => {
            CommitError::Constraint(e.message().to_owned())
        }
        _ => CommitError::Backend(err.to_string()),
    }
}

#[async_trait]
impl UnitOfWork for Store {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        if tx.is_empty() {
            return Ok(CommitReceipt::default());
        }
        for draft in &tx.new_effects {
            writes::validate_effect(draft)?;
        }
        // One writer connection: `BEGIN IMMEDIATE` gives mutual exclusion and makes the journal's
        // AUTOINCREMENT sequence equal commit order. If the write pool ever grows past one
        // connection, sequence assignment must change too.
        let mut dbtx = self
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let mut receipt = CommitReceipt::default();
        let mut mirrors: Vec<(JournalDraft, u64)> = Vec::new();
        for write in &tx.writes {
            writes::apply(&mut dbtx, write, &mut receipt, &mut mirrors).await?;
        }
        for (draft, legacy_event_seq) in &mirrors {
            receipt
                .journal_seqs
                .push(journal::insert(&mut dbtx, draft, Some(*legacy_event_seq)).await?);
        }
        for draft in &tx.journal {
            receipt
                .journal_seqs
                .push(journal::insert(&mut dbtx, draft, None).await?);
        }
        for draft in &tx.audit {
            let seq = insert_audit(&mut *dbtx, &draft.event)
                .await
                .map_err(map_store)?;
            receipt.audit_seqs.push(seq);
        }
        for draft in &tx.new_effects {
            effects::insert(&mut dbtx, draft).await?;
        }
        for completion in &tx.completed_effects {
            effects::complete(&mut dbtx, completion).await?;
        }
        dbtx.commit().await.map_err(backend)?;
        Ok(receipt)
    }
}
