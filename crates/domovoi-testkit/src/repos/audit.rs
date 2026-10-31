use async_trait::async_trait;
use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, Page, Paged, StoreError};

use super::{InMemoryStore, paginate};

fn matches(filter: &AuditFilter, event: &AuditEvent) -> bool {
    filter.actor.as_ref().is_none_or(|a| a == &event.actor)
        && filter.kind.as_ref().is_none_or(|k| k == &event.kind)
        && filter.subject.as_ref().is_none_or(|s| s == &event.subject)
        && filter.since.is_none_or(|t| event.at >= t)
}

#[async_trait]
impl AuditLog for InMemoryStore {
    async fn append(&self, event: &AuditEvent) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        state.audit.push(event.clone());
        Ok(state.audit.len() as u64)
    }

    async fn list(
        &self,
        filter: &AuditFilter,
        page: &Page,
    ) -> Result<Paged<AuditEvent>, StoreError> {
        let state = self.begin()?;
        let items = state
            .audit
            .iter()
            .zip(1_i128..)
            .filter(|(event, _)| matches(filter, event))
            .map(|(event, seq)| ((seq, String::new()), event.clone()))
            .collect();
        paginate(items, page)
    }
}
