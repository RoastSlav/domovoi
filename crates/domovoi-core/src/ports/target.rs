//! Resolving a target name to the canonical identity a plan binds (RW-34).
//!
//! Read-only: the resolver is a port so the engine can implement it beside its one read
//! boundary, and the hub can hand it out without either side depending on the other. It
//! never writes, never approves, and never reaches a tool by itself.

use async_trait::async_trait;

use crate::capability::{ResolvedTarget, TargetError, TargetQuery};

/// Resolves one target name to the identity a plan would bind.
#[async_trait]
pub trait TargetResolver: Send + Sync {
    /// The canonical identity of `query`, or why it is not provable now.
    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError>;
}
