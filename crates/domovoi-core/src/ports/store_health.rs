//! Whether the store can do its job right now, for the hub's readiness check.

use async_trait::async_trait;

use super::StoreError;

/// What `/readyz` asks the store. Each answer is a check name in that endpoint; the
/// error's text is logged and never returned to the caller.
#[async_trait]
pub trait StoreHealth: Send + Sync {
    /// Whether a write transaction can begin and roll back (the single writer is free and
    /// the disk accepts writes).
    async fn writable(&self) -> Result<(), StoreError>;
    /// Whether every embedded migration has been applied successfully.
    async fn migrations_current(&self) -> Result<bool, StoreError>;
}
