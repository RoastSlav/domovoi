//! Capability contracts: the hub's own description of what a capability is and how
//! dangerous it is.
//!
//! Contracts are owned here, not by an agent and not by a skill author. The built-in read
//! tools each get one typed contract derived from the closed `ReadTool` catalogue; a skill
//! is the data-defined family `skill.<name>`, whose declared class comes from the owner's
//! manifest but which the hub may only raise, through a floor. An unrecognised declaration
//! means `destructive`, and a skill can never be `contain`.
//!
//! [`ContractIndex`] validates a set and records which effect class each category belongs
//! to, which is what policy specificity and the evaluator rely on. The ceiling summary
//! types are the hub-side view of a host's unattended limits. [`Availability`] answers the
//! other half: which of these contracts one host can run right now, from the agent's own
//! report plus hub state. It is a report, never an approval.

pub mod availability;
pub mod builtin;
pub mod catalog;
mod ceiling;
mod contract;
mod effect;
mod ids;
mod index;
pub mod skill;
pub mod target;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_registry;
#[cfg(test)]
mod tests_skill;

pub use availability::{Availability, AvailabilityEntry, NotAllowedOnHost, UnavailableReason};
pub use builtin::{READ_OUTPUT_LIMIT, READ_TIMEOUT, builtin_contracts, contract_id_of};
pub use catalog::{
    CatalogHash, CatalogImplementation, CatalogPageData, CatalogQuery, CatalogSnapshot,
    CatalogueGap, CatalogueState, catalog_hash, is_hash,
};
pub use ceiling::{CeilingEntry, CeilingSummary, CeilingTargets};
pub use contract::{
    CapabilityContract, ConstrainableArg, ConstraintKind, ContractError, ContractSource,
};
pub use effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
pub use ids::{ContractCategory, ContractId, ContractVersion, FactId, TargetIdentity, TargetName};
pub use index::{ContractIndex, ContractMeta, IndexError};
pub use skill::{FloorError, SkillFloor, effective_effect, skill_contract, skill_input_schema};
pub use target::{
    ContainerId, ResolvedTarget, TargetError, TargetQuery, TargetUnresolved,
    canonical_container_identity, canonical_host_identity, validate_container_name,
};
