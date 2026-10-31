//! The triage cache and LLM usage totals.

use async_trait::async_trait;
use domovoi_core::ports::{StoreError, TriageCacheRepo, UsageRepo, UsageTotal};
use domovoi_core::{Fingerprint, TriageReport};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::codec::{from_json, int, json, ms, name, uint};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl TriageCacheRepo for Store {
    async fn get(
        &self,
        fingerprint: &Fingerprint,
        evidence_digest: &[u8; 32],
        since: Timestamp,
    ) -> Result<Option<TriageReport>, StoreError> {
        const SQL: &str = "SELECT report FROM triage_cache \
            WHERE fingerprint = ? AND evidence_digest = ? AND stored_at >= ?";
        let row: Option<(String,)> = sqlx::query_as(SQL)
            .bind(name(fingerprint)?)
            .bind(&evidence_digest[..])
            .bind(ms(since))
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(report,)| from_json(&report, "triage report"))
            .transpose()
    }

    async fn put(
        &self,
        fingerprint: &Fingerprint,
        evidence_digest: &[u8; 32],
        report: &TriageReport,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO triage_cache (fingerprint, evidence_digest, report, \
            stored_at) VALUES (?, ?, ?, ?) ON CONFLICT (fingerprint, evidence_digest) \
            DO UPDATE SET report = excluded.report, stored_at = excluded.stored_at";
        sqlx::query(SQL)
            .bind(name(fingerprint)?)
            .bind(&evidence_digest[..])
            .bind(json(report)?)
            .bind(ms(at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }
}

/// The UTC instants bounding the calendar month that contains `day`.
fn month_bounds(day: Date) -> Result<(Timestamp, Timestamp), StoreError> {
    let bad = |e: jiff::Error| StoreError::Backend(format!("month out of range: {e}"));
    let first = day.first_of_month();
    let next = first
        .checked_add(jiff::Span::new().months(1))
        .map_err(bad)?;
    let start = first.to_zoned(TimeZone::UTC).map_err(bad)?.timestamp();
    let end = next.to_zoned(TimeZone::UTC).map_err(bad)?.timestamp();
    Ok((start, end))
}

#[async_trait]
impl UsageRepo for Store {
    async fn record(
        &self,
        provider: &str,
        tokens_in: u64,
        tokens_out: u64,
        cost_micros: u64,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO llm_usage (provider, tokens_in, tokens_out, cost_micros, \
            at) VALUES (?, ?, ?, ?, ?)";
        sqlx::query(SQL)
            .bind(provider)
            .bind(int(tokens_in))
            .bind(int(tokens_out))
            .bind(int(cost_micros))
            .bind(ms(at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn month_total(&self, month: Date) -> Result<UsageTotal, StoreError> {
        const SQL: &str = "SELECT coalesce(sum(tokens_in), 0), coalesce(sum(tokens_out), 0), \
            coalesce(sum(cost_micros), 0) FROM llm_usage WHERE at >= ? AND at < ?";
        let (start, end) = month_bounds(month)?;
        let (tokens_in, tokens_out, cost): (i64, i64, i64) = sqlx::query_as(SQL)
            .bind(ms(start))
            .bind(ms(end))
            .fetch_one(&self.read)
            .await
            .map_err(db)?;
        Ok(UsageTotal {
            tokens_in: uint(tokens_in),
            tokens_out: uint(tokens_out),
            cost_micros: uint(cost),
        })
    }
}
