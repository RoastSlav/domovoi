//! Claiming due effects, dead-lettering the spent ones, and undoing interrupted claims.
//!
//! These run outside an owner's transaction: the claim pass is its own `BEGIN IMMEDIATE` so
//! the candidates it selects and the rows it rewrites cannot disagree.

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateKind, ClaimBatch, ClaimToken, ClaimedEffect, EffectKind, EffectQueue, EffectRecord,
    StoreError,
};
use jiff::{SignedDuration, Timestamp};
use sqlx::SqliteConnection;

use crate::codec::ms;
use crate::db::Store;
use crate::error::db;

use super::reads::{EFFECT_COLUMNS, EffectRow};

/// Why a claim pass gave up on an effect.
const ATTEMPT_LIMIT: &str = "attempt limit reached";
/// Why a claim pass gave up on an effect whose lease expired once too often.
const LEASE_LIMIT: &str = "lease expired too many times";

/// A `?` per kind, so the `IN` list is built from bound values and never from text.
fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Reads one row back after a claim pass rewrote it.
async fn read_effect(conn: &mut SqliteConnection, id: &str) -> Result<EffectRecord, StoreError> {
    let sql = format!("SELECT {EFFECT_COLUMNS} FROM effects WHERE id = ?");
    let row: EffectRow = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    row.into_record()
}

#[async_trait]
impl EffectQueue for Store {
    async fn claim_due(
        &self,
        runner: &str,
        kinds: &[EffectKind],
        aggregates: &[AggregateKind],
        now: Timestamp,
        lease: SignedDuration,
        limit: u16,
    ) -> Result<ClaimBatch, StoreError> {
        if kinds.is_empty() || aggregates.is_empty() || limit == 0 {
            return Ok(ClaimBatch::default());
        }
        let list = placeholders(kinds.len());
        let scope = placeholders(aggregates.len());
        // Due pending effects come first, expired claims second: a stale claim waits behind
        // work that has never started, even when its original due time is older. Each group is
        // ordered by due time and then insertion, and `limit` applies across both.
        let sql = format!(
            "SELECT id, status FROM effects \
             WHERE (status = 'pending' AND not_before <= ? AND kind IN ({list}) \
                    AND aggregate_type IN ({scope})) \
                OR (status = 'claimed' AND lease_until IS NOT NULL AND lease_until <= ? \
                    AND kind <> 'dispatch' AND kind IN ({list}) \
                    AND aggregate_type IN ({scope})) \
             ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, not_before, seq LIMIT ?"
        );
        let lease_until = now.checked_add(lease).unwrap_or(now);
        let mut dbtx = self.write.begin_with("BEGIN IMMEDIATE").await.map_err(db)?;
        let mut query = sqlx::query_as::<_, (String, String)>(sqlx::AssertSqlSafe(sql));
        for _branch in 0..2 {
            query = query.bind(ms(now));
            for kind in kinds {
                query = query.bind(kind.as_str());
            }
            for aggregate in aggregates {
                query = query.bind(aggregate.as_str());
            }
        }
        let candidates = query
            .bind(i64::from(limit))
            .fetch_all(&mut *dbtx)
            .await
            .map_err(db)?;

        let mut batch = ClaimBatch::default();
        for (id, selected_status) in candidates {
            let spent: Option<(i64, i64)> =
                sqlx::query_as("SELECT attempt, max_attempts FROM effects WHERE id = ?")
                    .bind(&id)
                    .fetch_optional(&mut *dbtx)
                    .await
                    .map_err(db)?;
            let Some((attempt, max_attempts)) = spent else {
                continue;
            };
            if attempt >= max_attempts {
                let reason = if selected_status == "claimed" {
                    LEASE_LIMIT
                } else {
                    ATTEMPT_LIMIT
                };
                sqlx::query(
                    "UPDATE effects SET status = 'dead', last_error = ?, completed_at = ?, \
                     claim_token = NULL, lease_until = NULL, runner_id = NULL WHERE id = ?",
                )
                .bind(reason)
                .bind(ms(now))
                .bind(&id)
                .execute(&mut *dbtx)
                .await
                .map_err(db)?;
                batch.dead.push(read_effect(&mut dbtx, &id).await?);
            } else {
                let token = ClaimToken::new();
                sqlx::query(
                    "UPDATE effects SET status = 'claimed', claim_token = ?, \
                     claim_generation = claim_generation + 1, lease_until = ?, runner_id = ?, \
                     attempt = attempt + 1 WHERE id = ?",
                )
                .bind(token.to_string())
                .bind(ms(lease_until))
                .bind(runner)
                .bind(&id)
                .execute(&mut *dbtx)
                .await
                .map_err(db)?;
                let record = read_effect(&mut dbtx, &id).await?;
                batch.claimed.push(ClaimedEffect { record, token });
            }
        }
        dbtx.commit().await.map_err(db)?;
        Ok(batch)
    }

    async fn revert_claimed_at_start(
        &self,
        aggregates: &[AggregateKind],
    ) -> Result<u64, StoreError> {
        if aggregates.is_empty() {
            return Ok(0);
        }
        let scope = placeholders(aggregates.len());
        let sql = format!(
            "UPDATE effects SET status = 'pending', claim_token = NULL, \
             claim_generation = claim_generation + 1, lease_until = NULL, runner_id = NULL \
             WHERE status = 'claimed' AND kind <> 'dispatch' AND aggregate_type IN ({scope})"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for aggregate in aggregates {
            query = query.bind(aggregate.as_str());
        }
        let done = query.execute(&self.write).await.map_err(db)?;
        Ok(done.rows_affected())
    }

    async fn parked_retries(&self, now: Timestamp) -> Result<u64, StoreError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM effects \
             WHERE status = 'pending' AND kind = 'build_context' \
               AND aggregate_type = 'operation' AND not_before > ? \
               AND dedup_key LIKE 'build:%:retry:%'",
        )
        .bind(ms(now))
        .fetch_one(&self.write)
        .await
        .map_err(db)?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    async fn pull_forward_retry(&self, now: Timestamp) -> Result<u64, StoreError> {
        let done = sqlx::query(
            "UPDATE effects SET not_before = ? WHERE id = ( \
               SELECT id FROM effects \
               WHERE status = 'pending' AND kind = 'build_context' \
                 AND aggregate_type = 'operation' AND not_before > ? \
                 AND dedup_key LIKE 'build:%:retry:%' \
               ORDER BY not_before, seq LIMIT 1)",
        )
        .bind(ms(now))
        .bind(ms(now))
        .execute(&self.write)
        .await
        .map_err(db)?;
        Ok(done.rows_affected())
    }
}
