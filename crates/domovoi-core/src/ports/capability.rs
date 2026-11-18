//! The read-only capability view the read and execution boundaries consult.
//!
//! The engine must ask *whether the hub would offer this here* without depending on the
//! hub that answers it, so this query API is a port: the hub's registry implements it, and
//! the testkit's fake does too. Nothing here evaluates policy, approves anything or
//! dispatches a run: "available" is not "approved", and the registry can never become a
//! second authorization decision (RW-10).

use async_trait::async_trait;

use crate::capability::{Availability, CatalogSnapshot, ContractId, ContractIndex, EffectClass};
use crate::id::HostId;
use crate::skill::{SkillName, SkillSummary};

/// The hub's answer to "is this capability available here, and if not, why not".
///
/// Implemented by the hub's registry, which combines the compiled contracts, the agent's
/// implementation list, the host's own state, hub enablement, the raise-only floor and the
/// agent's ceiling summary. It consults no policy rule and cannot approve or refuse a run
/// beyond the states named in section 6.1.
#[async_trait]
pub trait CapabilityRegistry: Send + Sync {
    /// Whether `contract` is available on `host`, and why not.
    ///
    /// May walk the agent's catalogue on a miss, and never holds an internal lock across
    /// an `.await`.
    async fn availability(&self, host: HostId, contract: &ContractId) -> Availability;

    /// This host's catalogue snapshot, walked as far as it can be.
    async fn catalog(&self, host: HostId) -> CatalogSnapshot;

    /// The hub's compiled contract set: the only source of hub-known contract ids.
    fn contracts(&self) -> &ContractIndex;

    /// The effective effect class of `skill` on `host`, its declared class raised by any
    /// hub floor, or `None` when the host does not announce that skill.
    fn effective_effect(&self, host: HostId, skill: &SkillName) -> Option<EffectClass>;

    /// Whether the hub enables `contract` on `host`. In-memory, so the hot path never
    /// waits on it.
    fn enabled(&self, host: HostId, contract: &ContractId) -> bool;

    /// The skills the hub may offer on `host`: announced by its agent **and** enabled
    /// here. The agent's own list stays unfiltered on [`AgentLink::skills`].
    fn enabled_skills(&self, host: HostId) -> Vec<SkillSummary>;
}
