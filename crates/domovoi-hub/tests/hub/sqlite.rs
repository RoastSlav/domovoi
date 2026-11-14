//! The SQLite store the crash sweep and the upgrade flow run on.
//!
//! The sweep needs a store that can report the references its repository ports cannot enumerate.
//! A trait implemented for a foreign type has to be implemented in the crate that defines the
//! trait or the type, and neither of those may know about the other, so the hub tests own this
//! wrapper: a local type that forwards every port to the store and answers the integrity read
//! with test-only queries. Nothing here is compiled into the hub.

use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    AckRepo, ActionFilter, ActionRepo, AggregateRef, AuditEvent, AuditFilter, AuditLog,
    CommitError, CommitReceipt, EffectRead, EffectRecord, EffectStatus, IncidentFilter,
    IncidentRepo, IncidentTimelineEntry, JournalEntry, JournalRead, Page, Paged, SignalRepo,
    StoreError, Transaction, UnitOfWork, Versioned, VersionedRead,
};
use domovoi_core::{
    AckRule, AckRuleId, ActionId, ActionRequest, Fingerprint, HostId, Incident, IncidentId,
    MaintenanceWindow, Note, NoteId, Signal, SkillName,
};
use domovoi_store::Store;
use domovoi_testkit::crash::{IntegrityRead, Orphan, OrphanStorage, check_invariants};
use jiff::Timestamp;

/// A SQLite store seen through every port, plus the references the ports cannot show.
#[derive(Clone)]
pub struct SqliteStore(Arc<Store>);

impl SqliteStore {
    /// Wraps an opened store.
    pub fn new(store: Store) -> Self {
        Self(Arc::new(store))
    }
}

impl Deref for SqliteStore {
    type Target = Store;

    fn deref(&self) -> &Store {
        &self.0
    }
}

#[async_trait]
impl UnitOfWork for SqliteStore {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        UnitOfWork::commit(&*self.0, tx).await
    }
}

#[async_trait]
impl VersionedRead for SqliteStore {
    async fn incident(&self, id: IncidentId) -> Result<Option<Versioned<Incident>>, StoreError> {
        VersionedRead::incident(&*self.0, id).await
    }

    async fn active_incidents(&self) -> Result<Vec<Versioned<Incident>>, StoreError> {
        VersionedRead::active_incidents(&*self.0).await
    }

    async fn note(&self, id: NoteId) -> Result<Option<Versioned<Note>>, StoreError> {
        VersionedRead::note(&*self.0, id).await
    }
}

#[async_trait]
impl JournalRead for SqliteStore {
    async fn entries_after(
        &self,
        after_seq: u64,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        JournalRead::entries_after(&*self.0, after_seq, limit).await
    }

    async fn entries_for(
        &self,
        aggregate: &AggregateRef,
        limit: u16,
    ) -> Result<Vec<JournalEntry>, StoreError> {
        JournalRead::entries_for(&*self.0, aggregate, limit).await
    }

    async fn head_seq(&self) -> Result<u64, StoreError> {
        JournalRead::head_seq(&*self.0).await
    }
}

#[async_trait]
impl EffectRead for SqliteStore {
    async fn effect(&self, id: domovoi_core::EffectId) -> Result<Option<EffectRecord>, StoreError> {
        EffectRead::effect(&*self.0, id).await
    }

    async fn effects_with_status(
        &self,
        status: EffectStatus,
        limit: u16,
    ) -> Result<Vec<EffectRecord>, StoreError> {
        EffectRead::effects_with_status(&*self.0, status, limit).await
    }
}

#[async_trait]
impl IncidentRepo for SqliteStore {
    async fn insert(&self, incident: &Incident) -> Result<(), StoreError> {
        IncidentRepo::insert(&*self.0, incident).await
    }

    async fn update(&self, incident: &Incident) -> Result<(), StoreError> {
        IncidentRepo::update(&*self.0, incident).await
    }

    async fn get(&self, id: IncidentId) -> Result<Option<Incident>, StoreError> {
        IncidentRepo::get(&*self.0, id).await
    }

    async fn find_active_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Incident>, StoreError> {
        IncidentRepo::find_active_by_fingerprint(&*self.0, fingerprint).await
    }

    async fn find_recent_resolved_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
        since: Timestamp,
    ) -> Result<Option<Incident>, StoreError> {
        IncidentRepo::find_recent_resolved_by_fingerprint(&*self.0, fingerprint, since).await
    }

    async fn list(
        &self,
        filter: &IncidentFilter,
        page: &Page,
    ) -> Result<Paged<Incident>, StoreError> {
        IncidentRepo::list(&*self.0, filter, page).await
    }

    async fn discard_pending(&self, id: IncidentId) -> Result<(), StoreError> {
        IncidentRepo::discard_pending(&*self.0, id).await
    }

    async fn append_event(&self, entry: &IncidentTimelineEntry) -> Result<(), StoreError> {
        IncidentRepo::append_event(&*self.0, entry).await
    }

    async fn timeline(&self, id: IncidentId) -> Result<Vec<IncidentTimelineEntry>, StoreError> {
        IncidentRepo::timeline(&*self.0, id).await
    }

    async fn search(&self, text: &str, limit: u16) -> Result<Vec<Incident>, StoreError> {
        IncidentRepo::search(&*self.0, text, limit).await
    }
}

#[async_trait]
impl SignalRepo for SqliteStore {
    async fn insert(
        &self,
        signal: &Signal,
        incident: Option<IncidentId>,
    ) -> Result<(), StoreError> {
        SignalRepo::insert(&*self.0, signal, incident).await
    }

    async fn list_for_incident(
        &self,
        incident: IncidentId,
        limit: u16,
    ) -> Result<Vec<Signal>, StoreError> {
        SignalRepo::list_for_incident(&*self.0, incident, limit).await
    }

    async fn purge_older_than(&self, before: Timestamp) -> Result<u64, StoreError> {
        SignalRepo::purge_older_than(&*self.0, before).await
    }
}

#[async_trait]
impl ActionRepo for SqliteStore {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        ActionRepo::insert(&*self.0, action).await
    }

    async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError> {
        ActionRepo::update_from(&*self.0, action, expected).await
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::get(&*self.0, id).await
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        ActionRepo::list(&*self.0, filter, page).await
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::find_by_chat_token_hash(&*self.0, hash).await
    }

    async fn last_run_by_skill(
        &self,
        host: HostId,
    ) -> Result<BTreeMap<SkillName, Timestamp>, StoreError> {
        ActionRepo::last_run_by_skill(&*self.0, host).await
    }
}

#[async_trait]
impl AuditLog for SqliteStore {
    async fn append(&self, event: &AuditEvent) -> Result<u64, StoreError> {
        AuditLog::append(&*self.0, event).await
    }

    async fn list(
        &self,
        filter: &AuditFilter,
        page: &Page,
    ) -> Result<Paged<AuditEvent>, StoreError> {
        AuditLog::list(&*self.0, filter, page).await
    }
}

#[async_trait]
impl AckRepo for SqliteStore {
    async fn put_rule(&self, rule: &AckRule) -> Result<(), StoreError> {
        AckRepo::put_rule(&*self.0, rule).await
    }

    async fn delete_rule(&self, id: AckRuleId) -> Result<(), StoreError> {
        AckRepo::delete_rule(&*self.0, id).await
    }

    async fn list_rules(&self) -> Result<Vec<AckRule>, StoreError> {
        AckRepo::list_rules(&*self.0).await
    }

    async fn put_window(&self, window: &MaintenanceWindow) -> Result<(), StoreError> {
        AckRepo::put_window(&*self.0, window).await
    }

    async fn windows_for(&self, host: HostId) -> Result<Vec<MaintenanceWindow>, StoreError> {
        AckRepo::windows_for(&*self.0, host).await
    }

    async fn purge_windows_ended_before(&self, before: Timestamp) -> Result<u64, StoreError> {
        AckRepo::purge_windows_ended_before(&*self.0, before).await
    }
}

/// The integrity read: rows whose `incident_id` names no stored incident. The store never writes
/// one, so the sweep checks for it after every crash.
#[async_trait]
impl IntegrityRead for SqliteStore {
    async fn orphan_incident_refs(&self) -> Result<Vec<Orphan>, StoreError> {
        const TIMELINE: &str = "SELECT incident_id, COUNT(*) FROM incident_events \
             WHERE NOT EXISTS (SELECT 1 FROM incidents i WHERE i.id = incident_id) \
             GROUP BY incident_id";
        const LINKS: &str = "SELECT incident_id, COUNT(*) FROM incident_signals \
             WHERE NOT EXISTS (SELECT 1 FROM incidents i WHERE i.id = incident_id) \
             GROUP BY incident_id";

        let mut orphans = Vec::new();
        for (storage, sql) in [
            (OrphanStorage::Timeline, TIMELINE),
            (OrphanStorage::SignalLink, LINKS),
        ] {
            let rows: Vec<(String, i64)> = sqlx::query_as(sql)
                .fetch_all(self.0.pool_for_tests())
                .await
                .map_err(|err| StoreError::Backend(err.to_string()))?;
            for (incident, count) in rows {
                orphans.push(Orphan {
                    storage,
                    incident: incident.parse().map_err(|_| {
                        StoreError::Backend(format!("incident id {incident} is not valid"))
                    })?,
                    count: usize::try_from(count)
                        .map_err(|_| StoreError::Backend("negative reference count".to_owned()))?,
                });
            }
        }
        orphans.sort();
        Ok(orphans)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use domovoi_core::HostId;
    use domovoi_store::StoreOptions;
    use domovoi_testkit::build::{T0, open_incident, signal};
    use sqlx::ConnectOptions;

    use super::*;

    /// Opens a store in the temporary directory the test owns for its lifetime.
    async fn open_store(dir: &Path) -> SqliteStore {
        let path = dir.join("integrity.db");
        SqliteStore::new(
            Store::open(&path, StoreOptions::beside(&path, T0))
                .await
                .expect("the database opens"),
        )
    }

    /// A valid store reports nothing, and rows naming an incident nobody stored are reported and
    /// counted by the invariants.
    #[tokio::test]
    async fn sqlite_integrity_read_reports_orphan_references() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("integrity.db");
        let store = open_store(dir.path()).await;

        let incident = open_incident(HostId::new(), "disk.smart", None).expect("a valid incident");
        IncidentRepo::insert(&store, &incident)
            .await
            .expect("the incident is stored");
        let linked = signal(HostId::new(), "net.link", None).expect("a valid signal");
        let signal_id = linked.id();
        SignalRepo::insert(&store, &linked, None)
            .await
            .expect("the signal is stored");

        assert!(
            store
                .orphan_incident_refs()
                .await
                .expect("the references are readable")
                .is_empty(),
            "a store the product wrote holds no dangling reference"
        );
        check_invariants(&store)
            .await
            .expect("a store the product wrote satisfies the invariants");

        // Rows that name an incident nobody stored. The store's own connections enforce the
        // timeline's foreign key, so they go in through a throwaway setup connection with the
        // constraint off, the way a damaged database would carry them; the store itself keeps
        // its settings and its behaviour.
        let missing = IncidentId::new();
        let mut raw = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .foreign_keys(false)
            .connect()
            .await
            .expect("a raw test connection");
        sqlx::query(
            "INSERT INTO incident_events (incident_id, at, kind, summary, detail) \
             VALUES (?, ?, 'status.changed', 'left behind', 'null')",
        )
        .bind(missing.to_string())
        .bind(T0.as_millisecond())
        .execute(&mut raw)
        .await
        .expect("an orphan timeline entry");
        sqlx::query("INSERT INTO incident_signals (incident_id, signal_id) VALUES (?, ?)")
            .bind(missing.to_string())
            .bind(signal_id.to_string())
            .execute(&mut raw)
            .await
            .expect("an orphan signal link");
        sqlx::Connection::close(raw)
            .await
            .expect("the raw connection closes");

        let orphans = store
            .orphan_incident_refs()
            .await
            .expect("the references are readable");
        assert_eq!(
            orphans,
            vec![
                Orphan {
                    storage: OrphanStorage::Timeline,
                    incident: missing,
                    count: 1,
                },
                Orphan {
                    storage: OrphanStorage::SignalLink,
                    incident: missing,
                    count: 1,
                },
            ]
        );

        let violations = check_invariants(&store).await.unwrap_err();
        assert_eq!(violations.len(), 2, "{violations:?}");
        assert!(
            violations
                .iter()
                .all(|violation| violation.starts_with("I3:")),
            "{violations:?}"
        );
        assert!(
            violations[0].contains(&missing.to_string()) && violations[0].contains("timeline"),
            "{violations:?}"
        );
        assert!(
            violations[1].contains(&missing.to_string()) && violations[1].contains("signal"),
            "{violations:?}"
        );
    }
}
