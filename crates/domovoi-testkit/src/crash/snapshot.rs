//! An id-free picture of the durable state.

use domovoi_core::ports::{ActionFilter, AuditFilter, IncidentFilter, Page, StoreError};

use super::SweepStore;

/// Id-free picture of the durable state: sorted so equal states compare equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// (check key, status name) of every incident, sorted. Versions are checked by
    /// invariants, not snapshots.
    pub incidents: Vec<(String, String)>,
    /// (skill name, status name, has_chat_token) of every action.
    pub actions: Vec<(String, String, bool)>,
    /// Number of signals linked to an incident.
    pub signals: usize,
    /// (kind) of every timeline entry, sorted.
    pub timeline_kinds: Vec<String>,
    /// (kind) of every audit row, sorted.
    pub audit_kinds: Vec<String>,
    /// (aggregate kind, journal kind) of every journal row, sorted.
    pub journal_kinds: Vec<(String, String)>,
}

impl Snapshot {
    /// Reads the whole durable state through `store` and returns its id-free picture.
    pub async fn capture<S: SweepStore>(store: &S) -> Result<Self, StoreError> {
        let incidents = all_incidents(store).await?;
        let mut signals = 0;
        let mut timeline_kinds = Vec::new();
        for incident in &incidents {
            let linked = store
                .list_for_incident(incident.id(), Page::MAX_LIMIT)
                .await?;
            signals += linked.len();
            timeline_kinds.extend(
                store
                    .timeline(incident.id())
                    .await?
                    .into_iter()
                    .map(|entry| entry.kind),
            );
        }

        let mut actions = all_actions(store).await?;
        actions.sort();

        let mut audit_kinds = all_audit_kinds(store).await?;
        audit_kinds.sort();

        let mut journal_kinds = all_journal_kinds(store).await?;
        journal_kinds.sort();

        let mut incidents: Vec<(String, String)> = incidents
            .into_iter()
            .map(|incident| {
                (
                    incident.check().as_str().to_owned(),
                    incident.status().name().to_owned(),
                )
            })
            .collect();
        incidents.sort();
        timeline_kinds.sort();

        Ok(Self {
            incidents,
            actions,
            signals,
            timeline_kinds,
            audit_kinds,
            journal_kinds,
        })
    }
}

/// Every stored incident, following cursor pages to the end.
pub(super) async fn all_incidents<S: SweepStore>(
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

/// Every stored action as its id-free snapshot row.
async fn all_actions<S: SweepStore>(store: &S) -> Result<Vec<(String, String, bool)>, StoreError> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got =
            domovoi_core::ports::ActionRepo::list(store, &ActionFilter::default(), &page).await?;
        items.extend(got.items.into_iter().map(|action| {
            (
                action.skill().as_str().to_owned(),
                action.status().name().to_owned(),
                action.chat_token_hash().is_some(),
            )
        }));
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(items),
        }
    }
}

/// Every audit row's kind, following cursor pages.
pub(super) async fn all_audit_kinds<S: SweepStore>(store: &S) -> Result<Vec<String>, StoreError> {
    let mut items = Vec::new();
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got =
            domovoi_core::ports::AuditLog::list(store, &AuditFilter::default(), &page).await?;
        items.extend(got.items.into_iter().map(|event| event.kind));
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return Ok(items),
        }
    }
}

/// Every journal row as (aggregate kind, journal kind), following sequence pages.
async fn all_journal_kinds<S: SweepStore>(store: &S) -> Result<Vec<(String, String)>, StoreError> {
    let mut items = Vec::new();
    let mut after = 0;
    loop {
        let batch = store.entries_after(after, Page::MAX_LIMIT).await?;
        if batch.is_empty() {
            return Ok(items);
        }
        let last = batch.last().map(|entry| entry.seq);
        items.extend(
            batch
                .into_iter()
                .map(|entry| (entry.aggregate.kind.as_str().to_owned(), entry.kind)),
        );
        match last {
            Some(seq) => after = seq,
            None => return Ok(items),
        }
    }
}
