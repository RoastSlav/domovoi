//! The readiness port for the in-memory store: always writable, always current, unless
//! a test injects a failure with [`InMemoryStore::fail_next`].

use async_trait::async_trait;
use domovoi_core::ports::{StoreError, StoreHealth};

use super::InMemoryStore;

#[async_trait]
impl StoreHealth for InMemoryStore {
    async fn writable(&self) -> Result<(), StoreError> {
        self.begin().map(|_| ())
    }

    async fn migrations_current(&self) -> Result<bool, StoreError> {
        self.begin().map(|_| true)
    }
}
