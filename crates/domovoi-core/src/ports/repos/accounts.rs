//! Settings, web UI accounts and sessions.

use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

use super::StoreError;
use crate::id::UserId;
use crate::user::{Session, User};

/// Runtime settings changed from the web UI, as JSON values by key.
#[async_trait]
pub trait SettingsRepo: Send + Sync {
    /// The value for `key`.
    async fn get(&self, key: &str) -> Result<Option<Value>, StoreError>;

    /// Sets `key`, replacing any previous value.
    async fn set(&self, key: &str, value: &Value) -> Result<(), StoreError>;
}

/// Web UI accounts.
#[async_trait]
pub trait UserRepo: Send + Sync {
    /// Stores a new account. [`StoreError::Conflict`] when the id or username is taken.
    async fn create(&self, user: &User) -> Result<(), StoreError>;

    /// The account with `id`.
    async fn get(&self, id: UserId) -> Result<Option<User>, StoreError>;

    /// The account with exactly this username (case-sensitive).
    async fn get_by_username(&self, username: &str) -> Result<Option<User>, StoreError>;

    /// Replaces an account's password hash.
    async fn set_password_hash(&self, id: UserId, password_hash: &str) -> Result<(), StoreError>;

    /// How many accounts exist; zero means first-start setup is still open.
    async fn count(&self) -> Result<u64, StoreError>;
}

/// Login sessions, looked up by the hash of the session cookie.
#[async_trait]
pub trait SessionRepo: Send + Sync {
    /// Stores a new session. [`StoreError::Conflict`] when the token hash is already stored.
    async fn create(&self, session: &Session) -> Result<(), StoreError>;

    /// The session for `token_hash`, or `None` when unknown or expired at `now`
    /// (`now >= expires_at`).
    async fn get(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<Option<Session>, StoreError>;

    /// Deletes one session (logout). Deleting an unknown session succeeds.
    async fn delete(&self, token_hash: &[u8; 32]) -> Result<(), StoreError>;

    /// Deletes every session of `user` and returns how many were deleted.
    async fn delete_for_user(&self, user: UserId) -> Result<u64, StoreError>;

    /// Deletes every session expired at `now` and returns how many were deleted.
    async fn purge_expired(&self, now: Timestamp) -> Result<u64, StoreError>;
}
