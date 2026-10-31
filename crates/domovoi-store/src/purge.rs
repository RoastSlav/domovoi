//! Retention: deleting what the policy no longer keeps. Incidents, the audit log and
//! digest facts are never purged.

use domovoi_core::ports::StoreError;
use jiff::{SignedDuration, Timestamp};

use crate::codec::ms;
use crate::db::Store;
use crate::error::db;

/// How long each kind of record is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Signals, by when they were observed.
    pub signals: SignedDuration,
    /// Incident timeline entries, by when they happened.
    pub timeline: SignedDuration,
    /// Cached triage reports, by when they were stored.
    pub triage: SignedDuration,
    /// Advisories, from when they were resolved.
    pub resolved_advisories: SignedDuration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            signals: SignedDuration::from_hours(24 * 30),
            timeline: SignedDuration::from_hours(24 * 90),
            triage: SignedDuration::from_hours(24 * 90),
            resolved_advisories: SignedDuration::from_hours(24 * 30),
        }
    }
}

/// How many rows one purge deleted, per table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Signals.
    pub signals: u64,
    /// Incident timeline entries.
    pub timeline: u64,
    /// Cached triage reports.
    pub triage: u64,
    /// Resolved advisories.
    pub advisories: u64,
    /// Maintenance windows that have ended.
    pub windows: u64,
    /// Expired web sessions.
    pub sessions: u64,
}

fn cutoff(now: Timestamp, keep: SignedDuration) -> i64 {
    ms(now.checked_sub(keep).unwrap_or(Timestamp::MIN))
}

impl Store {
    async fn delete(&self, sql: &'static str, before: i64) -> Result<u64, StoreError> {
        let done = sqlx::query(sql)
            .bind(before)
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }

    /// Deletes everything `policy` no longer keeps at `now`.
    pub async fn purge(
        &self,
        now: Timestamp,
        policy: &RetentionPolicy,
    ) -> Result<PurgeReport, StoreError> {
        let report = PurgeReport {
            signals: self
                .delete(
                    "DELETE FROM signals WHERE observed_at < ?",
                    cutoff(now, policy.signals),
                )
                .await?,
            timeline: self
                .delete(
                    "DELETE FROM incident_events WHERE at < ?",
                    cutoff(now, policy.timeline),
                )
                .await?,
            triage: self
                .delete(
                    "DELETE FROM triage_cache WHERE stored_at < ?",
                    cutoff(now, policy.triage),
                )
                .await?,
            advisories: self
                .delete(
                    "DELETE FROM advisories WHERE resolved_at IS NOT NULL AND resolved_at < ?",
                    cutoff(now, policy.resolved_advisories),
                )
                .await?,
            windows: self
                .delete(
                    "DELETE FROM maintenance_windows WHERE ends_at <= ?",
                    ms(now),
                )
                .await?,
            sessions: self
                .delete("DELETE FROM sessions WHERE expires_at <= ?", ms(now))
                .await?,
        };
        tracing::info!(?report, "purged old records");
        Ok(report)
    }
}
