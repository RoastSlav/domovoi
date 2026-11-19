//! The journal as a change stream: the pruned floor and a wait for newer rows.

use async_trait::async_trait;
use domovoi_core::ports::{JournalFeed, StoreError};

use crate::codec::uint;
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl JournalFeed for Store {
    async fn floor_seq(&self) -> Result<u64, StoreError> {
        let floor: i64 =
            sqlx::query_scalar("SELECT pruned_through_seq FROM journal_floor WHERE id = 1")
                .fetch_one(&self.read)
                .await
                .map_err(db)?;
        Ok(uint(floor))
    }

    async fn wait_after(&self, seq: u64) {
        let mut rx = self.head.subscribe();
        let _ = rx.wait_for(|head| *head > seq).await;
    }
}
