//! The triage cache and LLM usage accounting.

use async_trait::async_trait;
use jiff::Timestamp;
use jiff::civil::Date;

use super::StoreError;
use crate::fingerprint::Fingerprint;
use crate::triage::TriageReport;

/// Cached triage reports, so an unchanged incident is not triaged twice.
///
/// An entry is identified by fingerprint and evidence digest (SHA-256 of the evidence the
/// report was based on).
#[async_trait]
pub trait TriageCacheRepo: Send + Sync {
    /// The cached report, if one was stored at or after `since`.
    async fn get(
        &self,
        fingerprint: &Fingerprint,
        evidence_digest: &[u8; 32],
        since: Timestamp,
    ) -> Result<Option<TriageReport>, StoreError>;

    /// Stores a report, replacing any entry with the same fingerprint and digest.
    async fn put(
        &self,
        fingerprint: &Fingerprint,
        evidence_digest: &[u8; 32],
        report: &TriageReport,
        at: Timestamp,
    ) -> Result<(), StoreError>;
}

/// Token and cost totals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageTotal {
    /// Prompt tokens.
    pub tokens_in: u64,
    /// Generated tokens.
    pub tokens_out: u64,
    /// Cost in millionths of a US dollar.
    pub cost_micros: u64,
}

/// LLM usage records, for the monthly budget.
#[async_trait]
pub trait UsageRepo: Send + Sync {
    /// Appends one usage record.
    async fn record(
        &self,
        provider: &str,
        tokens_in: u64,
        tokens_out: u64,
        cost_micros: u64,
        at: Timestamp,
    ) -> Result<(), StoreError>;

    /// Sums every record whose `at` falls in the UTC calendar month containing `month`
    /// (any day of the month). Zero when there are none.
    async fn month_total(&self, month: Date) -> Result<UsageTotal, StoreError>;
}
