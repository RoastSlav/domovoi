//! References that the repository ports cannot enumerate.
//!
//! Every port reads an incident's timeline and its signal links *per existing incident*, so a
//! reference left behind by an incident that no longer exists is invisible through them. The
//! crash invariants must see those, so the sweep's store also answers [`IntegrityRead`].
//!
//! This is test-only support: no product port exposes it, the runtime never asks for it, and
//! the SQLite implementation lives in the hub tests next to the database it reads.

use async_trait::async_trait;
use domovoi_core::IncidentId;
use domovoi_core::ports::StoreError;

/// Which storage a dangling reference lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrphanStorage {
    /// An entry in an incident's timeline.
    Timeline,
    /// A signal linked to an incident.
    SignalLink,
}

/// References to one incident that is not stored, counted per storage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Orphan {
    /// Where the references are.
    pub storage: OrphanStorage,
    /// The incident that is not stored.
    pub incident: IncidentId,
    /// How many references name it.
    pub count: usize,
}

/// Reads the references that the repository ports cannot enumerate.
///
/// Part of [`SweepStore`](super::SweepStore) because the invariants must see them; nothing in the
/// product needs a scan for dangling references.
#[async_trait]
pub trait IntegrityRead: Send + Sync {
    /// Every reference to an incident that is not stored, sorted by storage and incident.
    async fn orphan_incident_refs(&self) -> Result<Vec<Orphan>, StoreError>;
}

/// The in-memory store answers from its own state: the timeline is keyed by the incident it
/// belongs to and each signal carries its link.
#[async_trait]
impl IntegrityRead for crate::InMemoryStore {
    async fn orphan_incident_refs(&self) -> Result<Vec<Orphan>, StoreError> {
        let (timeline, links, incidents) = self.integrity_refs();
        let mut orphans = Vec::new();
        for (storage, refs) in [
            (OrphanStorage::Timeline, timeline),
            (OrphanStorage::SignalLink, links),
        ] {
            for (incident, count) in refs {
                if !incidents.contains(&incident) {
                    orphans.push(Orphan {
                        storage,
                        incident,
                        count,
                    });
                }
            }
        }
        orphans.sort();
        Ok(orphans)
    }
}
