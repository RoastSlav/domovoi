use async_trait::async_trait;
use domovoi_core::ports::{NotifyStateRepo, StoreError};
use domovoi_core::{IncidentId, NotifyState};
use jiff::Timestamp;

use super::InMemoryStore;

#[async_trait]
impl NotifyStateRepo for InMemoryStore {
    async fn get(&self, incident: IncidentId) -> Result<Option<NotifyState>, StoreError> {
        Ok(self.begin()?.notify.get(&incident).map(|(s, _)| s.clone()))
    }

    async fn put(
        &self,
        incident: IncidentId,
        state: &NotifyState,
        at: Timestamp,
    ) -> Result<(), StoreError> {
        self.begin()?.notify.insert(incident, (state.clone(), at));
        Ok(())
    }

    async fn list_held(&self) -> Result<Vec<(IncidentId, NotifyState)>, StoreError> {
        let state = self.begin()?;
        let mut held: Vec<_> = state
            .notify
            .iter()
            .filter(|(_, (s, _))| s.is_held())
            .map(|(id, (s, at))| (*at, *id, s.clone()))
            .collect();
        held.sort_by_key(|h| std::cmp::Reverse((h.0, h.1)));
        Ok(held.into_iter().map(|(_, id, s)| (id, s)).collect())
    }

    async fn delete(&self, incident: IncidentId) -> Result<(), StoreError> {
        self.begin()?.notify.remove(&incident);
        Ok(())
    }
}
