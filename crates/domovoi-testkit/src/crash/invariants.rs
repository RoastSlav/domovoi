//! The cross-table invariants F1 guarantees.

use std::collections::BTreeMap;

use domovoi_core::IncidentId;
use domovoi_core::ports::{
    AggregateRef, IncidentFilter, JournalKind, JournalRead, Page, StoreError,
};

use super::SweepStore;
use super::integrity::OrphanStorage;

/// Rows that predate the journal, so the invariants cannot expect their journal rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exempt {
    /// How many timeline entries per incident predate the journal.
    pub legacy_timeline: BTreeMap<IncidentId, usize>,
}

impl Exempt {
    /// Nothing is exempt.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

/// Checks the cross-table invariants of F1. Returns every violation found, not just the
/// first.
pub async fn check_invariants<S: SweepStore>(store: &S) -> Result<(), Vec<String>> {
    check_invariants_except(store, &Exempt::none()).await
}

/// Checks the F1 invariants, allowing the rows named in `exempt` to predate the journal.
pub async fn check_invariants_except<S: SweepStore>(
    store: &S,
    exempt: &Exempt,
) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();

    let incidents = match all_incidents(store).await {
        Ok(incidents) => incidents,
        Err(err) => return Err(vec![format!("I3: the incidents could not be read: {err}")]),
    };
    for incident in &incidents {
        let timeline = match domovoi_core::ports::IncidentRepo::timeline(store, incident.id()).await
        {
            Ok(timeline) => timeline,
            Err(err) => {
                violations.push(format!(
                    "I3: the timeline for incident {} could not be read: {err}",
                    incident.id()
                ));
                continue;
            }
        };
        let entries = match JournalRead::entries_for(
            store,
            &AggregateRef::incident(incident.id()),
            Page::MAX_LIMIT,
        )
        .await
        {
            Ok(entries) => entries,
            Err(err) => {
                violations.push(format!(
                    "I3: the journal for incident {} could not be read: {err}",
                    incident.id()
                ));
                continue;
            }
        };
        let written = entries
            .iter()
            .filter(|entry| entry.kind == JournalKind::IncidentTimeline.as_str())
            .count();
        let legacy = exempt
            .legacy_timeline
            .get(&incident.id())
            .copied()
            .unwrap_or(0);
        let expected = timeline.len().saturating_sub(legacy);
        if expected != written {
            violations.push(format!(
                "I3: incident {} has {expected} journaled timeline entries but {written} journal rows",
                incident.id()
            ));
        }
    }

    // References left behind by an incident that no longer exists cannot be reached through
    // the ports, which read a timeline and its links only for an incident that exists, so the
    // store reports them itself.
    match store.orphan_incident_refs().await {
        Ok(orphans) => {
            for orphan in orphans {
                let (one, many) = match orphan.storage {
                    OrphanStorage::Timeline => ("timeline entry", "timeline entries"),
                    OrphanStorage::SignalLink => ("signal link", "signal links"),
                };
                let (what, verb) = if orphan.count == 1 {
                    (one, "refers")
                } else {
                    (many, "refer")
                };
                violations.push(format!(
                    "I3: {} {what} {verb} to incident {}, which is not stored",
                    orphan.count, orphan.incident
                ));
            }
        }
        Err(err) => violations.push(format!(
            "I3: the orphan references could not be read: {err}"
        )),
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Every incident, following cursor pages.
async fn all_incidents<S: SweepStore>(
    store: &S,
) -> Result<Vec<domovoi_core::Incident>, StoreError> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = domovoi_core::ports::IncidentRepo::list(store, &IncidentFilter::default(), &page)
            .await?;
        items.extend(got.items);
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(items),
        }
    }
}
