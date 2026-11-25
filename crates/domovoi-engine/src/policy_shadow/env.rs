//! The port through which the recorder asks the hub what it knows about a host.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::capability::{
    CeilingSummary, ContractId, ContractIndex, ContractMeta, FactId, TargetIdentity, TargetName,
};

use super::recorded::GapReason;
use crate::policy::FactState;

/// A host's own name and identity, the target of every host-level contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTarget {
    /// The name rules match on.
    pub name: TargetName,
    /// The canonical identity permits and pinning bind to.
    pub identity: TargetIdentity,
}

/// What the hub knows of a host's unattended ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeilingReading {
    /// The agent reported a valid ceiling summary.
    Reported(CeilingSummary),
    /// The host is connected but reports none (or an unreadable one): the ceiling is
    /// treated as absent, which the evaluator reads as "does not cover".
    Absent,
    /// There is no live session to ask.
    HostNotConnected,
}

/// What the hub knows of a host's containment posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainmentReading {
    /// The contract is not a containment contract.
    NotApplicable,
    /// No verified attestation can be had; a helper-dependent contract fails closed.
    /// A verified reading arrives with the helper.
    Unavailable,
}

/// The autonomy switches as the hub reads them now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutonomyFacts {
    /// Whether the owner paused autonomy: no new permit, no dispatch of an issued one.
    pub paused: bool,
    /// The rules whose circuit breaker is open, derived from the executions and the resets.
    pub suspended_rules: BTreeSet<String>,
}

/// What the hub knows of the autonomy switches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutonomyReading {
    /// The state read from the store.
    Reported(AutonomyFacts),
}

/// Read-only view of the hub used to build evaluator inputs. The engine never imports the
/// hub: the hub implements this over its registry and target resolver, and the testkit
/// provides a fake.
///
/// Every method is read-only, never waits on a host beyond the registry's own deadlines, and
/// never fails: an unknown answer is a value, and the recorder substitutes the fail-closed
/// input and lists the substitution as a gap.
#[async_trait]
pub trait ShadowEnvironment: Send + Sync {
    /// The contract as the hub sees it on `host` now: a compiled contract, or a
    /// `skill.<name>` contract built from the host's announced skills with the registry's
    /// effective class. `None` when it cannot be established.
    async fn contract(&self, host: HostId, id: &ContractId) -> Option<ContractMeta>;

    /// The host's own target. Host-level contracts (every skill) use it.
    async fn host_target(&self, host: HostId) -> Result<HostTarget, GapReason>;

    /// The unit the owner or the model typed as `typed`, as the helper reports it: the name
    /// rules match on and the canonical identity a permit binds. An environment with no helper
    /// path answers that the unit cannot be resolved.
    async fn unit_target(
        &self,
        _host: HostId,
        _typed: &TargetName,
    ) -> Result<HostTarget, GapReason> {
        Err(GapReason::Unresolvable)
    }

    /// The canonical identity of the unit a pinned subject names by its target key, or `None`
    /// when it cannot be built.
    async fn unit_identity(&self, _host: HostId, _unit: &str) -> Option<TargetIdentity> {
        None
    }

    /// The host's unattended ceiling.
    async fn ceiling(&self, host: HostId) -> CeilingReading;

    /// Whether the host's agent runs as root; `None` without a live hello. The recorder
    /// reads `None` as root, the safe direction.
    async fn runs_as_root(&self, host: HostId) -> Option<bool>;

    /// The safety facts the hub knows among `wanted`. Empty until fact producers exist;
    /// absent facts are read as unknown.
    async fn facts(&self, host: HostId, wanted: &BTreeSet<FactId>) -> BTreeMap<FactId, FactState>;

    /// The host's containment posture.
    async fn containment(&self, host: HostId) -> ContainmentReading;

    /// The autonomy switches.
    async fn autonomy(&self) -> AutonomyReading;

    /// Compiled contracts plus one representative contract per skill name announced
    /// anywhere (the strictest effective class wins). Publication review runs over it.
    async fn publication_index(&self) -> ContractIndex;

    /// The parsed ceiling of every host that reported a valid one; absent and malformed
    /// summaries are omitted.
    async fn ceilings(&self) -> BTreeMap<HostId, CeilingSummary>;
}
