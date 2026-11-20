//! How a store is reopened after a simulated crash.

use std::sync::Arc;

use async_trait::async_trait;

/// Reopens durable storage after a simulated crash.
#[async_trait]
pub trait Reopen<S>: Send + Sync {
    /// Closes `old` if the backend needs it and returns the store as a freshly started
    /// process would see it. In memory: the same instance. SQLite: close the pools and
    /// open the file again.
    async fn reopen(&self, old: Arc<S>) -> Arc<S>;
}

/// Reopens nothing: the in-memory store survives as the same instance.
pub struct InMemoryReopen;

#[async_trait]
impl Reopen<crate::InMemoryStore> for InMemoryReopen {
    async fn reopen(&self, old: Arc<crate::InMemoryStore>) -> Arc<crate::InMemoryStore> {
        old
    }
}
