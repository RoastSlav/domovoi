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
//! types are the hub-side view of a host's unattended limits.

pub mod builtin;
mod ceiling;
mod contract;
mod effect;
mod ids;
mod index;
mod rules;
mod schema;
mod schema_argument;
pub mod skill;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ceiling;
#[cfg(test)]
mod tests_ids;
#[cfg(test)]
mod tests_schema;
#[cfg(test)]
mod tests_skill;

pub use builtin::{READ_OUTPUT_LIMIT, READ_TIMEOUT, builtin_contracts, contract_id_of};
pub use ceiling::{CeilingEntry, CeilingError, CeilingSummary, CeilingTargets};
pub use contract::{
    CapabilityContract, ConstrainableArg, ConstraintKind, ContractError, ContractSource,
};
pub use effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
pub use ids::{ContractCategory, ContractId, ContractVersion, FactId, TargetIdentity, TargetName};
pub use index::{ContractIndex, ContractMeta, IndexError};
pub use schema::{
    SchemaError, declared_type, declares_argument, requires_argument, stated_bounds,
    validate_schema,
};
pub use skill::{
    FloorError, SkillContractError, SkillFloor, effective_effect, skill_contract,
    skill_input_schema,
};
