//! Resolving a target name to the canonical identity a plan binds.
//!
//! Read-only: the resolver is a port so the engine can implement it beside its one read
//! boundary, and the hub can hand it out without either side depending on the other. It
//! never writes, never approves, and never reaches a tool by itself.

use async_trait::async_trait;

use crate::capability::{ResolvedTarget, TargetError, TargetName, TargetQuery, TargetUnresolved};
use crate::id::HostId;
use crate::unit::ResolvedUnit;

/// Resolves one target name to the identity a plan would bind.
#[async_trait]
pub trait TargetResolver: Send + Sync {
    /// The canonical identity of `query`, or why it is not provable now.
    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError>;

    /// The unit `name` on `host` as the helper reports it: the canonical identity to pin, the
    /// identity hash to bind and the card to show. A resolver without a helper path answers
    /// that the identity cannot be proven.
    async fn resolve_unit(
        &self,
        host: HostId,
        name: &TargetName,
    ) -> Result<ResolvedUnit, TargetError> {
        Err(TargetError::Unresolved {
            query: TargetQuery::Unit {
                host,
                name: name.clone(),
            },
            reason: TargetUnresolved::UnitIdentityUnproven,
        })
    }
}
