//! The readiness questions a running store can answer about itself.

use async_trait::async_trait;
use domovoi_core::ports::{StoreError, StoreHealth};

use crate::db::{MIGRATOR, Store};
use crate::error::db;

#[async_trait]
impl StoreHealth for Store {
    async fn writable(&self) -> Result<(), StoreError> {
        let mut conn = self.write.acquire().await.map_err(db)?;
        // A transaction that is rolled back never reaches the disk, so it cannot tell a full
        // disk from a healthy one. Rewriting the file header's `user_version` with its own
        // value commits one page: the same write a login or an audit row needs.
        let (version,): (i64,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&mut *conn)
            .await
            .map_err(db)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(db)?;
        let wrote = sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {version}"
        )))
        .execute(&mut *conn)
        .await
        .map_err(db);
        let ended = sqlx::query(if wrote.is_ok() { "COMMIT" } else { "ROLLBACK" })
            .execute(&mut *conn)
            .await
            .map_err(db);
        if ended.is_err() {
            // A commit that failed (disk full) can leave the transaction open.
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }
        wrote?;
        ended?;
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
