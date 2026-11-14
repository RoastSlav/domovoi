//! The triage cache and LLM usage accounting.

use async_trait::async_trait;
use jiff::Timestamp;
use jiff::civil::Date;

use super::StoreError;
use crate::fingerprint::Fingerprint;
use crate::ports::llm::{LlmError, UsageSource};
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
    /// Cost in millionths of a US dollar, summing only the known prices.
    pub cost_micros: u64,
    /// Rows of the month that are a real response whose price is unknown. Their cost
    /// cannot be measured, so a monthly budget cannot be checked against them.
    pub unpriced_responses: u64,
}

/// How a model call ended, as stored in `llm_usage.outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    /// The call returned a response.
    Ok,
    /// The provider asked to slow down.
    RateLimited,
    /// The credentials were refused.
    Auth,
    /// The call did not finish within the hard per-call deadline.
    Timeout,
    /// A network or protocol failure.
    Transport,
    /// The answer could not be parsed.
    InvalidResponse,
    /// The provider was not usable.
    Unavailable,
    /// The provider refused a capability the request carried.
    CapabilityRejected,
}

impl CallOutcome {
    /// The stored name, for example `ok` or `rate_limited`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::RateLimited => "rate_limited",
            Self::Auth => "auth",
            Self::Timeout => "timeout",
            Self::Transport => "transport",
            Self::InvalidResponse => "invalid_response",
            Self::Unavailable => "unavailable",
            Self::CapabilityRejected => "capability_rejected",
        }
    }
}

impl From<&LlmError> for CallOutcome {
    fn from(err: &LlmError) -> Self {
        match err {
            LlmError::Transport(_) => Self::Transport,
            LlmError::RateLimited { .. } => Self::RateLimited,
            LlmError::Auth => Self::Auth,
            LlmError::Timeout { .. } => Self::Timeout,
            LlmError::InvalidResponse(_) => Self::InvalidResponse,
            LlmError::CapabilityRejected { .. } => Self::CapabilityRejected,
            LlmError::Unavailable(_) => Self::Unavailable,
        }
    }
}

/// One model call to record, successful or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageEntry<'a> {
    /// Provider id, as configured; never model output.
    pub provider: &'a str,
    /// Configured model name, when the adapter knows it.
    pub model: Option<&'a str>,
    /// How the call ended.
    pub outcome: CallOutcome,
    /// Prompt tokens, zero for a call that produced no response.
    pub tokens_in: u64,
    /// Generated tokens, zero for a call that produced no response.
    pub tokens_out: u64,
    /// `None` means the price is unknown. Never use `Some(0)` for unknown: zero is a
    /// genuine free model.
    pub cost_micros: Option<u64>,
    /// `Some` exactly when the call produced a response; `None` for a failed call that
    /// produced none. A failed call is never labelled `Reported`.
    pub usage_source: Option<UsageSource>,
    /// How long the call took, in milliseconds.
    pub latency_ms: u64,
    /// When the call started.
    pub at: Timestamp,
}

/// LLM usage records, for the monthly budget.
#[async_trait]
pub trait UsageRepo: Send + Sync {
    /// Appends one model call's usage, whatever its outcome.
    async fn record_call(&self, entry: &UsageEntry<'_>) -> Result<(), StoreError>;

    /// Appends one successful, provider-priced usage record. Kept for callers that only
    /// know tokens and cost.
    async fn record(
        &self,
        provider: &str,
        tokens_in: u64,
        tokens_out: u64,
        cost_micros: u64,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.record_call(&UsageEntry {
            provider,
            model: None,
            outcome: CallOutcome::Ok,
            tokens_in,
            tokens_out,
            cost_micros: Some(cost_micros),
            usage_source: Some(UsageSource::Reported),
            latency_ms: 0,
            at,
        })
        .await
    }

    /// Sums every record whose `at` falls in the UTC calendar month containing `month`
    /// (any day of the month). Zero when there are none. Unknown costs are counted as
    /// [`UsageTotal::unpriced_responses`], never as zero cost.
    async fn month_total(&self, month: Date) -> Result<UsageTotal, StoreError>;
}
