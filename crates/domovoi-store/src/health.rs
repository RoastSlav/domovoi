//! The readiness questions a running store can answer about itself.

use async_trait::async_trait;
use domovoi_core::ports::{StoreError, StoreHealth};

use crate::db::{MIGRATOR, Store};
use crate::error::db;

#[async_trait]
impl StoreHealth for Store {
    async fn writable(&self) -> Result<(), StoreError> {
        let mut conn = self.write.acquire().await.map_err(db)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        // Rolling back, never committing: the probe writes nothing but proves the writer
        // is free and the disk accepts writes.
        sqlx::query("ROLLBACK")
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn migrations_current(&self) -> Result<bool, StoreError> {
        let rows: Vec<(i64,)> =
            sqlx::query_as("SELECT version FROM _sqlx_migrations WHERE success = 1")
                .fetch_all(&self.read)
                .await
                .map_err(db)?;
        let applied: Vec<i64> = rows.into_iter().map(|(version,)| version).collect();
        Ok(MIGRATOR
            .iter()
            .all(|migration| applied.contains(&migration.version)))
    }
}
