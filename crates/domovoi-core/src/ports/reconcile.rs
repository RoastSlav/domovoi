//! Told when a host's agent authenticated, so its dispatched work can be reconciled.

use async_trait::async_trait;

use crate::id::HostId;

/// Told when a host's agent authenticated, so the host's dispatched work can be
/// reconciled against that agent's journal. Implemented by the action service.
#[async_trait]
pub trait HostReconciler: Send + Sync {
    /// Reconciles the host's dispatched executions. Idempotent; safe to call on every
    /// authenticated `hello`.
    async fn reconcile_host(&self, host: HostId);
}
