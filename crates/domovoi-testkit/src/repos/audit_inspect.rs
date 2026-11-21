//! The audit inspection port for the in-memory store. It lists its events without chain
//! fields: it does not chain, has no anchor and cannot verify.

use async_trait::async_trait;
use domovoi_core::audit_chain::{AuditAnchor, ChainRow, Hash, VerifyReport, VerifyScope};
use domovoi_core::ports::{AuditHistory, AuditInspect, AuditRowFilter, Page, Paged, StoreError};

use super::audit::matches;
use super::{InMemoryStore, paginate};

#[async_trait]
impl AuditInspect for InMemoryStore {
    async fn rows(
        &self,
        filter: &AuditRowFilter,
        page: &Page,
    ) -> Result<Paged<ChainRow>, StoreError> {
        let state = self.begin()?;
        let items = if filter.history == Some(AuditHistory::Legacy) {
            Vec::new()
        } else {
            state
                .audit
                .iter()
                .zip(1_u64..)
                .filter(|(event, _)| matches(&filter.events, event))
                .map(|(event, seq)| {
                    (
                        (i128::from(seq), String::new()),
                        ChainRow {
                            seq,
                            prev_hash: None,
                            hash: None,
                            event: event.clone(),
                        },
                    )
                })
                .collect()
        };
        paginate(items, page)
    }

    async fn anchor(&self) -> Result<Option<AuditAnchor>, StoreError> {
        self.begin().map(|_| None)
    }

    async fn head(&self) -> Result<Option<(u64, Hash)>, StoreError> {
        self.begin().map(|_| None)
    }

    async fn verify(&self, _scope: VerifyScope) -> Result<VerifyReport, StoreError> {
        Err(StoreError::Backend(
            "the in-memory store keeps no audit chain".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domovoi_core::ports::{AuditEvent, AuditLog};
    use jiff::Timestamp;
    use serde_json::Value;

    fn event(kind: &str) -> AuditEvent {
        AuditEvent {
            at: Timestamp::UNIX_EPOCH,
            actor: "system:test".to_owned(),
            kind: kind.to_owned(),
            subject: "s".to_owned(),
            detail: Value::Null,
        }
    }

    #[tokio::test]
    async fn in_memory_audit_inspect_lists_events_without_a_chain() {
        let store = InMemoryStore::default();
        store.append(&event("a")).await.expect("append");
        store.append(&event("b")).await.expect("append");
        let page = Page::first(10);
        let rows = store
            .rows(&AuditRowFilter::default(), &page)
            .await
            .expect("rows");
        assert_eq!(rows.items.len(), 2);
        assert!(rows.items.iter().all(|r| r.hash.is_none()));
        assert!(store.anchor().await.expect("anchor").is_none());
        assert!(store.head().await.expect("head").is_none());
        let err = store.verify(VerifyScope::Full).await.expect_err("no chain");
        assert!(err.to_string().contains("no audit chain"));
        let legacy = AuditRowFilter {
            history: Some(AuditHistory::Legacy),
            ..AuditRowFilter::default()
        };
        assert!(
            store
                .rows(&legacy, &page)
                .await
                .expect("rows")
                .items
                .is_empty()
        );
    }
}
