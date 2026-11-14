//! The persisted hourly model-run window.

use async_trait::async_trait;
use domovoi_core::ports::{AdmissionOutcome, AdmissionPurpose, AdmissionRepo, StoreError};
use jiff::{SignedDuration, Timestamp};

use crate::codec::from_ms;
use crate::db::Store;
use crate::error::db;

/// The starts inside `[since, ...)`, oldest first, read through `executor`.
async fn starts_newer_than(
    conn: &mut sqlx::SqliteConnection,
    since: i64,
) -> Result<Vec<Timestamp>, StoreError> {
    const SQL: &str = "SELECT started_at FROM llm_admission WHERE started_at > ? \
        ORDER BY started_at, id";
    let rows: Vec<(i64,)> = sqlx::query_as(SQL)
        .bind(since)
        .fetch_all(&mut *conn)
        .await
        .map_err(db)?;
    rows.into_iter().map(|(at,)| from_ms(at)).collect()
}

#[async_trait]
impl AdmissionRepo for Store {
    async fn admit(
        &self,
        purpose: AdmissionPurpose,
        room: u32,
        window: SignedDuration,
        now: Timestamp,
    ) -> Result<AdmissionOutcome, StoreError> {
        const PRUNE: &str = "DELETE FROM llm_admission WHERE started_at < ?";
        const INSERT: &str = "INSERT INTO llm_admission (purpose, started_at) VALUES (?, ?)";
        let window_ms = i64::try_from(window.as_millis()).unwrap_or(i64::MAX);
        let now_ms = now.as_millisecond();
        let since = now_ms.saturating_sub(window_ms);
        let prune_before = now_ms.saturating_sub(window_ms.saturating_mul(2));
        let mut tx = self.write.begin().await.map_err(db)?;
        sqlx::query(PRUNE)
            .bind(prune_before)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let starts = starts_newer_than(&mut tx, since).await?;
        if starts.len() >= usize::try_from(room).unwrap_or(usize::MAX) {
            tx.commit().await.map_err(db)?;
            return Ok(AdmissionOutcome::Full { starts });
        }
        sqlx::query(INSERT)
            .bind(purpose.as_str())
            .bind(now_ms)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        tx.commit().await.map_err(db)?;
        Ok(AdmissionOutcome::Admitted)
    }

    async fn starts_since(&self, since: Timestamp) -> Result<Vec<Timestamp>, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        starts_newer_than(&mut conn, since.as_millisecond()).await
    }
}
