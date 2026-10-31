use async_trait::async_trait;
use domovoi_core::ports::{
    IncidentFilter, IncidentRepo, IncidentTimelineEntry, Page, Paged, SignalRepo, StoreError,
};
use domovoi_core::{Fingerprint, Incident, IncidentId, IncidentStatus, Signal};
use jiff::Timestamp;

use super::{InMemoryStore, State, conflict, limit, paginate, word_score, words};

fn is_active(incident: &Incident) -> bool {
    !matches!(incident.status(), IncidentStatus::Resolved)
}

/// Whether storing `incident` would leave two active incidents with one fingerprint.
fn second_active(state: &State, incident: &Incident) -> bool {
    is_active(incident)
        && state.incidents.values().any(|other| {
            other.id() != incident.id()
                && other.fingerprint() == incident.fingerprint()
                && is_active(other)
        })
}

#[async_trait]
impl SignalRepo for InMemoryStore {
    async fn insert(
        &self,
        signal: &Signal,
        incident: Option<IncidentId>,
    ) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if state.signals.contains_key(&signal.id()) {
            return Err(conflict("signal id exists"));
        }
        state
            .signals
            .insert(signal.id(), (signal.clone(), incident));
        Ok(())
    }

    async fn list_for_incident(
        &self,
        incident: IncidentId,
        max: u16,
    ) -> Result<Vec<Signal>, StoreError> {
        let state = self.begin()?;
        let mut signals: Vec<Signal> = state
            .signals
            .values()
            .filter(|(_, linked)| *linked == Some(incident))
            .map(|(signal, _)| signal.clone())
            .collect();
        signals.sort_by(|a, b| {
            b.observed_at()
                .cmp(&a.observed_at())
                .then_with(|| b.id().cmp(&a.id()))
        });
        signals.truncate(limit(max));
        Ok(signals)
    }

    async fn purge_older_than(&self, before: Timestamp) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        let count = state.signals.len();
        state
            .signals
            .retain(|_, (signal, _)| signal.observed_at() >= before);
        Ok((count - state.signals.len()) as u64)
    }
}

#[async_trait]
impl IncidentRepo for InMemoryStore {
    async fn insert(&self, incident: &Incident) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if state.incidents.contains_key(&incident.id()) {
            return Err(conflict("incident id exists"));
        }
        if second_active(&state, incident) {
            return Err(conflict("active incident with this fingerprint exists"));
        }
        state.incidents.insert(incident.id(), incident.clone());
        Ok(())
    }

    async fn update(&self, incident: &Incident) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if !state.incidents.contains_key(&incident.id()) {
            return Err(StoreError::NotFound);
        }
        if second_active(&state, incident) {
            return Err(conflict("active incident with this fingerprint exists"));
        }
        state.incidents.insert(incident.id(), incident.clone());
        Ok(())
    }

    async fn get(&self, id: IncidentId) -> Result<Option<Incident>, StoreError> {
        Ok(self.begin()?.incidents.get(&id).cloned())
    }

    async fn find_active_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Incident>, StoreError> {
        Ok(self
            .begin()?
            .incidents
            .values()
            .find(|i| i.fingerprint() == *fingerprint && is_active(i))
            .cloned())
    }

    async fn find_recent_resolved_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
        since: Timestamp,
    ) -> Result<Option<Incident>, StoreError> {
        Ok(self
            .begin()?
            .incidents
            .values()
            .filter(|i| i.fingerprint() == *fingerprint && !is_active(i))
            .filter(|i| i.resolved_at().is_some_and(|at| at >= since))
            .max_by_key(|i| (i.resolved_at(), i.id()))
            .cloned())
    }

    async fn list(
        &self,
        filter: &IncidentFilter,
        page: &Page,
    ) -> Result<Paged<Incident>, StoreError> {
        let state = self.begin()?;
        let items = state
            .incidents
            .values()
            .filter(|i| filter.host.is_none_or(|h| i.host() == h))
            .filter(|i| filter.active.is_none_or(|active| is_active(i) == active))
            .filter(|i| filter.opened_since.is_none_or(|t| i.opened_at() >= t))
            .map(|i| {
                (
                    (i.opened_at().as_nanosecond(), i.id().to_string()),
                    i.clone(),
                )
            })
            .collect();
        paginate(items, page)
    }

    async fn discard_pending(&self, id: IncidentId) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        match state.incidents.get(&id) {
            None => return Err(StoreError::NotFound),
            Some(incident) if incident.status() != IncidentStatus::Pending => {
                return Err(conflict("only pending incidents are discarded"));
            }
            Some(_) => {}
        }
        state.incidents.remove(&id);
        state.timeline.remove(&id);
        for (_, link) in state.signals.values_mut() {
            if *link == Some(id) {
                *link = None;
            }
        }
        Ok(())
    }

    async fn append_event(&self, entry: &IncidentTimelineEntry) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if !state.incidents.contains_key(&entry.incident) {
            return Err(StoreError::NotFound);
        }
        state
            .timeline
            .entry(entry.incident)
            .or_default()
            .push(entry.clone());
        Ok(())
    }

    async fn timeline(&self, id: IncidentId) -> Result<Vec<IncidentTimelineEntry>, StoreError> {
        Ok(self.begin()?.timeline.get(&id).cloned().unwrap_or_default())
    }

    async fn search(&self, text: &str, max: u16) -> Result<Vec<Incident>, StoreError> {
        let query = words(text);
        let state = self.begin()?;
        let mut hits: Vec<Incident> = state
            .incidents
            .values()
            .filter(|i| word_score(&query, &words(i.title())).is_some())
            .cloned()
            .collect();
        hits.sort_by(|a, b| {
            b.opened_at()
                .cmp(&a.opened_at())
                .then_with(|| b.id().cmp(&a.id()))
        });
        hits.truncate(limit(max));
        Ok(hits)
    }
}
