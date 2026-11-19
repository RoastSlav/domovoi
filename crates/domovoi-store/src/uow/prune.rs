//! The one supported way to delete journal rows.

use domovoi_core::ports::StoreError;

use crate::db::Store;
use crate::error::db;

impl Store {
    /// Deletes every journal row with `seq <= seq` and raises the pruned floor to at least `seq`,
    /// in one transaction. Returns how many rows were deleted.
    ///
    /// Any future retention code that deletes journal rows by age or kind must raise
    /// `pruned_through_seq` to at least the highest `seq` it deleted, in the same transaction,
    /// or event streams cannot tell a client that it missed rows.
    ///
    /// # Errors
    /// A backend failure; nothing is deleted then.
    pub async fn prune_journal_through(&self, seq: u64) -> Result<u64, StoreError> {
        let bound = i64::try_from(seq).unwrap_or(i64::MAX);
        let mut tx = self.write.begin().await.map_err(db)?;
        let deleted = sqlx::query("DELETE FROM journal WHERE seq <= ?")
            .bind(bound)
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        sqlx::query(
            "UPDATE journal_floor SET pruned_through_seq = MAX(pruned_through_seq, ?) WHERE id = 1",
        )
        .bind(bound)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        tx.commit().await.map_err(db)?;
        self.head.send_modify(|head| *head = (*head).max(seq));
        Ok(deleted)
    }
}
