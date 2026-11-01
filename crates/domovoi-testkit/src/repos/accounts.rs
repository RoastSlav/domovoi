use async_trait::async_trait;
use domovoi_core::ports::{SessionRepo, SettingsRepo, StoreError, UserRepo};
use domovoi_core::{Session, User, UserId};
use jiff::Timestamp;
use serde_json::Value;

use super::{InMemoryStore, conflict};

#[async_trait]
impl SettingsRepo for InMemoryStore {
    async fn get(&self, key: &str) -> Result<Option<Value>, StoreError> {
        Ok(self.begin()?.settings.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &Value) -> Result<(), StoreError> {
        self.begin()?.settings.insert(key.to_owned(), value.clone());
        Ok(())
    }
}

#[async_trait]
impl UserRepo for InMemoryStore {
    async fn create(&self, user: &User) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if state.users.contains_key(&user.id) {
            return Err(conflict("user id exists"));
        }
        if state.users.values().any(|u| u.username == user.username) {
            return Err(conflict("username taken"));
        }
        state.users.insert(user.id, user.clone());
        Ok(())
    }

    async fn get(&self, id: UserId) -> Result<Option<User>, StoreError> {
        Ok(self.begin()?.users.get(&id).cloned())
    }

    async fn get_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        Ok(self
            .begin()?
            .users
            .values()
            .find(|u| u.username == username)
            .cloned())
    }

    async fn set_password_hash(&self, id: UserId, password_hash: &str) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let user = state.users.get_mut(&id).ok_or(StoreError::NotFound)?;
        password_hash.clone_into(&mut user.password_hash);
        Ok(())
    }

    async fn count(&self) -> Result<u64, StoreError> {
        Ok(self.begin()?.users.len() as u64)
    }
}

#[async_trait]
impl SessionRepo for InMemoryStore {
    async fn create(&self, session: &Session) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if state.sessions.contains_key(&session.token_hash) {
            return Err(conflict("session exists"));
        }
        state.sessions.insert(session.token_hash, session.clone());
        Ok(())
    }

    async fn get(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<Option<Session>, StoreError> {
        Ok(self
            .begin()?
            .sessions
            .get(token_hash)
            .filter(|s| now < s.expires_at)
            .cloned())
    }

    async fn extend(&self, token_hash: &[u8; 32], expires_at: Timestamp) -> Result<(), StoreError> {
        if let Some(session) = self.begin()?.sessions.get_mut(token_hash) {
            session.expires_at = expires_at;
        }
        Ok(())
    }

    async fn delete(&self, token_hash: &[u8; 32]) -> Result<(), StoreError> {
        self.begin()?.sessions.remove(token_hash);
        Ok(())
    }

    async fn delete_for_user(&self, user: UserId) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        let count = state.sessions.len();
        state.sessions.retain(|_, s| s.user != user);
        Ok((count - state.sessions.len()) as u64)
    }

    async fn purge_expired(&self, now: Timestamp) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        let count = state.sessions.len();
        state.sessions.retain(|_, s| now < s.expires_at);
        Ok((count - state.sessions.len()) as u64)
    }
}
