use async_trait::async_trait;
use domovoi_core::ports::{StoreError, TriageCacheRepo, UsageEntry, UsageRepo, UsageTotal};
use domovoi_core::{Fingerprint, TriageReport};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use super::InMemoryStore;
use super::UsageRow;

#[async_trait]
impl TriageCacheRepo for InMemoryStore {
    async fn get(
        &self,
        fingerprint: &Fingerprint,
        evidence_digest: &[u8; 32],
        since: Timestamp,
    ) -> Result<Option<TriageReport>, StoreError> {
        Ok(self
            .begin()?
            .triage_cache
            .get(&(*fingerprint, *evidence_digest))
            .filter(|(_, at)| *at >= since)
            .map(|(report, _)| report.clone()))
    }

    async fn put(
        &self,
        fingerprint: &Fingerprint,
        evidence_digest: &[u8; 32],
        report: &TriageReport,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.begin()?
            .triage_cache
            .insert((*fingerprint, *evidence_digest), (report.clone(), at));
        Ok(())
    }
}

#[async_trait]
impl UsageRepo for InMemoryStore {
    async fn record_call(&self, entry: &UsageEntry<'_>) -> Result<(), StoreError> {
        self.begin()?.usage.push(UsageRow {
            provider: entry.provider.to_owned(),
            model: entry.model.map(str::to_owned),
            outcome: entry.outcome,
            tokens_in: entry.tokens_in,
            tokens_out: entry.tokens_out,
            cost_micros: entry.cost_micros,
            usage_source: entry.usage_source,
            latency_ms: entry.latency_ms,
            at: entry.at,
        });
        Ok(())
    }

    async fn month_total(&self, month: Date) -> Result<UsageTotal, StoreError> {
        let state = self.begin()?;
        let mut total = UsageTotal::default();
        for row in &state.usage {
            let day = row.at.to_zoned(TimeZone::UTC).date();
            if day.year() == month.year() && day.month() == month.month() {
                total.tokens_in += row.tokens_in;
                total.tokens_out += row.tokens_out;
                total.cost_micros += row.cost_micros.unwrap_or(0);
                if row.usage_source.is_some() && row.cost_micros.is_none() {
                    total.unpriced_responses += 1;
                }
            }
        }
        Ok(total)
    }
}
