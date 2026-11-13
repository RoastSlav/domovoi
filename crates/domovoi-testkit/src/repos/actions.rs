use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::ports::{ActionFilter, ActionRepo, Page, Paged, RAN_STATUSES, StoreError};
use domovoi_core::{ActionId, ActionRequest, HostId, SkillName};
use jiff::Timestamp;

use super::{InMemoryStore, conflict, paginate};

#[async_trait]
impl ActionRepo for InMemoryStore {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if state.actions.contains_key(&action.id()) {
            return Err(conflict("action id exists"));
        }
        if let Some(hash) = action.chat_token_hash()
            && state
                .actions
                .values()
                .any(|a| a.chat_token_hash() == Some(hash))
        {
            return Err(conflict("chat token hash exists"));
        }
        state.actions.insert(action.id(), action.clone());
        Ok(())
    }

    async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let stored = state
            .actions
            .get(&action.id())
            .ok_or(StoreError::NotFound)?;
        if stored.status().name() != expected {
            return Err(conflict("the action changed meanwhile"));
        }
        *state
            .actions
            .get_mut(&action.id())
            .ok_or(StoreError::NotFound)? = action.clone();
        Ok(())
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        Ok(self.begin()?.actions.get(&id).cloned())
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        let state = self.begin()?;
        let items = state
            .actions
            .values()
            .filter(|a| filter.incident.is_none_or(|i| a.incident() == i))
            .filter(|a| filter.host.is_none_or(|h| a.host() == h))
            .filter(|a| {
                filter
                    .status
                    .as_deref()
                    .is_none_or(|s| a.status().name() == s)
            })
            .filter(|a| {
                filter
                    .decided
                    .is_none_or(|decided| (a.status().name() != "proposed") == decided)
            })
            .filter(|a| filter.updated_since.is_none_or(|t| a.updated_at() >= t))
            .filter(|a| filter.expires_before.is_none_or(|due| a.expires_at() < due))
            .map(|a| {
                (
                    (a.created_at().as_nanosecond(), a.id().to_string()),
                    a.clone(),
                )
            })
            .collect();
        paginate(items, page)
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        Ok(self
            .begin()?
            .actions
            .values()
            .find(|a| a.chat_token_hash() == Some(hash))
            .cloned())
    }

    async fn last_run_by_skill(
        &self,
        host: HostId,
    ) -> Result<BTreeMap<SkillName, Timestamp>, StoreError> {
        let state = self.begin()?;
        let mut last = BTreeMap::new();
        for action in state.actions.values() {
            if action.host() == host && RAN_STATUSES.contains(&action.status().name()) {
                let at = last
                    .entry(action.skill().clone())
                    .or_insert_with(|| action.updated_at());
                *at = (*at).max(action.updated_at());
            }
        }
        Ok(last)
    }
}
