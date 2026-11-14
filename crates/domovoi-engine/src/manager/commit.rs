//! One transaction per manager command: version bookkeeping and the write translation.

use std::collections::BTreeMap;

use domovoi_core::IncidentId;
use domovoi_core::ports::{CommitReceipt, Expected, StateWrite, Transaction};

use super::command::CommandError;
use super::decide::Write;
use domovoi_core::Incident;

/// The metric counting version conflicts the manager retried.
pub const WRITE_CONFLICTS_METRIC: &str = "domovoi_incident_write_conflicts_total";

/// Versions of the incidents the manager has read, keyed by incident id.
#[derive(Debug, Default, Clone)]
pub(crate) struct Versions(BTreeMap<IncidentId, u64>);

impl Versions {
    /// The version recorded for `id`, if the manager has read it.
    pub(crate) fn get(&self, id: IncidentId) -> Option<u64> {
        self.0.get(&id).copied()
    }

    /// Records `version` as the version last read for `id`.
    pub(crate) fn set(&mut self, id: IncidentId, version: u64) {
        self.0.insert(id, version);
    }

    /// Forgets `id`, for example once its incident is resolved or discarded.
    pub(crate) fn remove(&mut self, id: IncidentId) {
        self.0.remove(&id);
    }

    /// Forgets every recorded version, before reloading them all.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Translates `writes` into one transaction.
///
/// `cached` is the manager's version cache; `loaded` holds versions of incidents read for this
/// command only (reopened, triaged) and takes precedence over `cached`. Within the transaction
/// an incident written twice expects `version + 1` the second time; an incident inserted earlier
/// in the same transaction expects `Version(1)` when updated later.
pub(crate) fn build_transaction(
    writes: &[Write],
    cached: &Versions,
    loaded: &BTreeMap<IncidentId, u64>,
) -> Result<Transaction, CommandError> {
    let mut tx = Transaction::new();
    let mut tracked: BTreeMap<IncidentId, u64> = loaded.clone();
    for write in writes {
        let state_write = match write {
            Write::InsertSignal(signal, incident) => StateWrite::Signal {
                signal: signal.clone(),
                incident: *incident,
            },
            Write::InsertIncident(incident) => {
                tracked.insert(incident.id(), 1);
                StateWrite::Incident {
                    incident: incident.clone(),
                    expected: Expected::New,
                }
            }
            Write::UpdateIncident(incident) => {
                let version = known_version(incident.id(), &tracked, cached)?;
                tracked.insert(incident.id(), version + 1);
                StateWrite::Incident {
                    incident: incident.clone(),
                    expected: Expected::Version(version),
                }
            }
            Write::Timeline(entry) => StateWrite::IncidentEvent(entry.clone()),
            Write::DiscardPending(id) => {
                let version = known_version(*id, &tracked, cached)?;
                tracked.remove(id);
                StateWrite::DiscardPendingIncident {
                    id: *id,
                    expected: version,
                }
            }
        };
        tx = tx.write(state_write);
    }
    Ok(tx)
}

/// The version a write must name, from this command's reads or the cache.
fn known_version(
    id: IncidentId,
    tracked: &BTreeMap<IncidentId, u64>,
    cached: &Versions,
) -> Result<u64, CommandError> {
    tracked
        .get(&id)
        .copied()
        .or_else(|| cached.get(id))
        .ok_or_else(|| CommandError::Store(format!("no known version for incident {id}")))
}

/// Updates the version cache after a successful commit.
///
/// Every incident write produced one entry in `receipt.versions`, in write order. An incident
/// still active afterwards keeps its new version; one that resolved or was discarded loses its
/// entry.
pub(crate) fn record_versions(
    cache: &mut Versions,
    active: &[Incident],
    writes: &[Write],
    receipt: &CommitReceipt,
) {
    let mut committed = receipt.versions.iter();
    for write in writes {
        let incident = match write {
            Write::InsertIncident(incident) | Write::UpdateIncident(incident) => incident,
            Write::InsertSignal(..) | Write::Timeline(_) | Write::DiscardPending(_) => continue,
        };
        let Some((_, version)) = committed.next() else {
            return;
        };
        if active.iter().any(|i| i.id() == incident.id()) {
            cache.set(incident.id(), *version);
        } else {
            cache.remove(incident.id());
        }
    }
    for write in writes {
        if let Write::DiscardPending(id) = write {
            cache.remove(*id);
        }
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::ports::{AggregateRef, Expected, StateWrite};
    use domovoi_core::{HostId, Incident, IncidentId};
    use domovoi_testkit::build::{T0, open_incident};

    use super::super::decide::Write;
    use super::*;

    fn incident() -> Incident {
        open_incident(HostId::new(), "disk.smart", Some("/dev/sda")).unwrap()
    }

    fn loaded(id: IncidentId, version: u64) -> BTreeMap<IncidentId, u64> {
        BTreeMap::from([(id, version)])
    }

    fn incident_writes(tx: &Transaction) -> Vec<&StateWrite> {
        tx.writes
            .iter()
            .filter(|write| matches!(write, StateWrite::Incident { .. }))
            .collect()
    }

    #[test]
    fn maps_each_write_to_its_state_write() {
        let incident = incident();
        let id = incident.id();
        let signal = domovoi_testkit::build::signal(HostId::new(), "disk.smart", None).unwrap();
        let entry = domovoi_core::ports::IncidentTimelineEntry {
            incident: id,
            at: T0,
            kind: "incident.created".to_owned(),
            summary: "opened".to_owned(),
            detail: serde_json::Value::Null,
        };
        let pending = open_incident(HostId::new(), "net.link", None).unwrap();
        let writes = vec![
            Write::InsertSignal(signal.clone(), Some(id)),
            Write::InsertIncident(incident.clone()),
            Write::UpdateIncident(incident),
            Write::Timeline(entry.clone()),
            Write::DiscardPending(pending.id()),
        ];
        let mut cached = Versions::default();
        cached.set(pending.id(), 2);

        let tx = build_transaction(&writes, &cached, &BTreeMap::new()).unwrap();

        assert_eq!(tx.writes.len(), 5);
        assert!(matches!(
            &tx.writes[0],
            StateWrite::Signal { incident: Some(linked), .. } if *linked == id
        ));
        assert!(matches!(
            &tx.writes[1],
            StateWrite::Incident {
                expected: Expected::New,
                ..
            }
        ));
        assert!(matches!(
            &tx.writes[2],
            StateWrite::Incident {
                expected: Expected::Version(1),
                ..
            }
        ));
        assert!(matches!(
            &tx.writes[3],
            StateWrite::IncidentEvent(found) if *found == entry
        ));
        assert!(matches!(
            &tx.writes[4],
            StateWrite::DiscardPendingIncident { expected: 2, .. }
        ));
        assert!(tx.journal.is_empty(), "the store mirrors timeline entries");
        assert!(tx.audit.is_empty(), "the manager writes no audit rows");
        assert!(tx.new_effects.is_empty() && tx.completed_effects.is_empty());
    }

    #[test]
    fn second_update_in_one_command_expects_the_next_version() {
        let incident = incident();
        let writes = vec![
            Write::UpdateIncident(incident.clone()),
            Write::UpdateIncident(incident.clone()),
        ];
        let loaded = loaded(incident.id(), 1);
        let tx = build_transaction(&writes, &Versions::default(), &loaded).unwrap();

        let updates = incident_writes(&tx);
        assert!(matches!(
            updates[0],
            StateWrite::Incident {
                expected: Expected::Version(1),
                ..
            }
        ));
        assert!(matches!(
            updates[1],
            StateWrite::Incident {
                expected: Expected::Version(2),
                ..
            }
        ));
    }

    #[test]
    fn update_after_insert_expects_version_one() {
        let incident = incident();
        let writes = vec![
            Write::InsertIncident(incident.clone()),
            Write::UpdateIncident(incident),
        ];
        let tx = build_transaction(&writes, &Versions::default(), &BTreeMap::new()).unwrap();

        let writes = incident_writes(&tx);
        assert!(matches!(
            writes[0],
            StateWrite::Incident {
                expected: Expected::New,
                ..
            }
        ));
        assert!(matches!(
            writes[1],
            StateWrite::Incident {
                expected: Expected::Version(1),
                ..
            }
        ));
    }

    #[test]
    fn missing_version_is_an_error() {
        let incident = incident();
        let writes = vec![Write::UpdateIncident(incident)];
        let err = build_transaction(&writes, &Versions::default(), &BTreeMap::new()).unwrap_err();
        assert!(
            matches!(&err, CommandError::Store(message) if message.contains("no known version")),
            "{err:?}"
        );
    }

    #[test]
    fn a_discard_needs_a_known_version_too() {
        let id = IncidentId::new();
        let err = build_transaction(
            &[Write::DiscardPending(id)],
            &Versions::default(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(matches!(err, CommandError::Store(_)), "{err:?}");
    }

    #[test]
    fn receipt_versions_follow_the_incident_writes() {
        let first = incident();
        let second = incident();
        let mut cache = Versions::default();
        cache.set(first.id(), 1);
        cache.set(second.id(), 1);
        let receipt = CommitReceipt {
            versions: vec![
                (AggregateRef::incident(first.id()), 2),
                (AggregateRef::incident(second.id()), 2),
            ],
            ..CommitReceipt::default()
        };
        let writes = vec![
            Write::UpdateIncident(first.clone()),
            Write::InsertIncident(second.clone()),
        ];

        record_versions(&mut cache, std::slice::from_ref(&first), &writes, &receipt);

        assert_eq!(
            cache.get(first.id()),
            Some(2),
            "the active incident advances"
        );
        assert_eq!(
            cache.get(second.id()),
            None,
            "an incident no longer active loses its version"
        );
    }

    #[test]
    fn a_discard_removes_the_version_even_without_a_receipt_entry() {
        let pending = incident();
        let mut cache = Versions::default();
        cache.set(pending.id(), 1);
        let writes = vec![Write::DiscardPending(pending.id())];

        record_versions(&mut cache, &[], &writes, &CommitReceipt::default());

        assert_eq!(cache.get(pending.id()), None);
    }
}
