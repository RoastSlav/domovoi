use async_trait::async_trait;
use domovoi_core::ports::{StoreError, TriageCacheRepo, UsageRepo, UsageTotal};
use domovoi_core::{Fingerprint, TriageReport};
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use super::InMemoryStore;

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
    async fn record(
        &self,
        provider: &str,
        tokens_in: u64,
        tokens_out: u64,
        cost_micros: u64,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.begin()?
            .usage
            .push((provider.to_owned(), tokens_in, tokens_out, cost_micros, at));
        Ok(())
    }

    async fn month_total(&self, month: Date) -> Result<UsageTotal, StoreError> {
        let state = self.begin()?;
        let mut total = UsageTotal::default();
        for (_, tokens_in, tokens_out, cost, at) in &state.usage {
            let day = at.to_zoned(TimeZone::UTC).date();
            if day.year() == month.year() && day.month() == month.month() {
                total.tokens_in += tokens_in;
                total.tokens_out += tokens_out;
                total.cost_micros += cost;
            }
        }
        Ok(total)
    }
}
