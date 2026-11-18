//! The rules a contract's fields must agree with, and the boundary that runs them.
//!
//! Every contract the hub hands out has been through [`CapabilityContract::validate`]: the
//! built-in table, [`skill_contract`] and [`ContractIndex`] all call it, and the registry only
//! ever yields contracts that came through one of those paths. The rules are grouped by what they
//! tie together, and each returns one [`ContractError`] variant so a caller knows which field to
//! fix.
//!
//! Three conventions are checked here, where a hand-written contract from a later prompt would
//! otherwise slip through: the reserved `skill.` category namespace belongs to skills only, a
//! skill's category names its effect class, and a skill's id is exactly `skill.<skill name>`, so
//! the runner names no manifest that is not there. The built-in read convention, that a read
//! category ends in the segment `observe`, is checked where the built-ins are composed
//! ([`ContractIndex::with_builtins`]), because a rule about a category word cannot decide what a
//! future built-in family is called.

use std::collections::BTreeSet;

use jiff::SignedDuration;

use super::contract::{
    CapabilityContract, ContractError, ContractSource, ExpiryOwner, RollbackDeclaration,
};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, FactId};
use super::schema::{
    declared_type, declares_argument, requires_argument, stated_bounds, validate_schema,
};
use super::skill::{SkillFloor, effective_effect};

/// The largest output limit a contract may promise, in bytes.
const MAX_OUTPUT_LIMIT: u32 = 1_048_576;
/// The longest a contract may say one run lasts.
const MAX_TIMEOUT: SignedDuration = SignedDuration::from_secs(3600);
/// The longest a recheck may wait, which matches what a skill manifest may declare.
const MAX_RECHECK_DELAY: SignedDuration = SignedDuration::from_hours(1);

impl CapabilityContract {
    /// Checks that every field is consistent with the others and with the argument schema.
    ///
    /// The whole contract is refused when any rule fails, so a contract that contradicts itself
    /// never reaches the registry, the inventory or the policy evaluator.
    pub fn validate(&self) -> Result<(), ContractError> {
        if !(1..=MAX_OUTPUT_LIMIT).contains(&self.output_limit_bytes) {
            return Err(ContractError::OutputLimit {
                id: self.id.clone(),
                bytes: self.output_limit_bytes,
            });
        }
        if self.timeout <= SignedDuration::ZERO || self.timeout > MAX_TIMEOUT {
            return Err(ContractError::Timeout {
                id: self.id.clone(),
            });
        }
        for (field, text) in [("title", &self.title), ("description", &self.description)] {
            if text.trim().is_empty() {
                return Err(ContractError::BlankText {
                    id: self.id.clone(),
                    field,
                });
            }
        }
        source_rules(self)?;
        effect_rules(self)?;
        target_rules(self)?;
        constraint_rules(self)?;
        fact_rules(self)?;
        verification_rules(self)?;
        validate_schema(&self.input_schema).map_err(|reason| ContractError::SchemaInvalid {
            id: self.id.clone(),
            reason,
        })?;
        containment_rules(self)?;
        Ok(())
    }
}

/// Ties the id, the source, the category, the execution path and the target kind together.
///
/// A skill runs only through the skill runner, only ever owns a `skill.` id, never owns a
/// target kind of its own (the runner acts on the host), and may not be `contain`. A built-in
/// owns none of the `skill.` shapes.
fn source_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    match &contract.source {
        ContractSource::BuiltIn => {
            if contract.id.is_skill() || contract.execution == ExecutionPath::SkillRunner {
                return Err(ContractError::SkillSourceMismatch {
                    id: contract.id.clone(),
                });
            }
            if contract.category.is_skill_namespace() {
                return Err(ContractError::BuiltInSkillCategory {
                    id: contract.id.clone(),
                });
            }
            if contract.execution == ExecutionPath::Helper && contract.effect == EffectClass::Read {
                return Err(ContractError::HelperPathForRead {
                    id: contract.id.clone(),
                });
            }
            Ok(())
        }
        ContractSource::Skill { declared, floor } => {
            if !contract.id.is_skill()
                || contract.execution != ExecutionPath::SkillRunner
                || contract.target_kind != TargetKind::Host
            {
                return Err(ContractError::SkillSourceMismatch {
                    id: contract.id.clone(),
                });
            }
            if contract.id.skill_name().is_none() {
                return Err(ContractError::SkillIdNotCanonical {
                    id: contract.id.clone(),
                });
            }
            if matches!(declared, EffectClass::Contain)
                || contract.effect == EffectClass::Contain
                || matches!(floor, Some(EffectClass::Contain))
            {
                return Err(ContractError::SkillContain {
                    id: contract.id.clone(),
                });
            }
            let raised: Option<SkillFloor> = match floor {
                Some(class) => {
                    Some(
                        SkillFloor::new(*class).map_err(|_| ContractError::SkillContain {
                            id: contract.id.clone(),
                        })?,
                    )
                }
                None => None,
            };
            let expected = effective_effect(*declared, raised);
            if expected != contract.effect {
                return Err(ContractError::SkillEffectMismatch {
                    id: contract.id.clone(),
                    expected,
                    found: contract.effect,
                });
            }
            if let Some(category) = ContractCategory::skill(contract.effect)
                && contract.category != category
            {
                return Err(ContractError::SkillCategoryMismatch {
                    id: contract.id.clone(),
                    expected: category.clone(),
                    found: contract.category.clone(),
                });
            }
            Ok(())
        }
    }
}

/// Ties the effect class to the fields only a capability that changes state may set.
fn effect_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    if contract.effect == EffectClass::Read {
        if let Some(field) = side_effect_field(contract) {
            return Err(ContractError::ReadHasSideEffectFields {
                id: contract.id.clone(),
                field,
            });
        }
        return Ok(());
    }
    if contract.effect == EffectClass::Contain && contract.execution != ExecutionPath::Helper {
        return Err(ContractError::ContainNeedsHelper {
            id: contract.id.clone(),
        });
    }
    if contract.effect == EffectClass::Mutate
        && contract.reversibility != Reversibility::Reversible
        && contract.idempotency != Idempotency::Idempotent
    {
        return Err(ContractError::MutateMustBeUndoable {
            id: contract.id.clone(),
        });
    }
    if contract.reversibility == Reversibility::NotApplicable
        || (!is_skill(contract) && contract.verification == VerificationHint::None)
    {
        return Err(ContractError::BuiltInNeedsReversibilityAndVerification {
            id: contract.id.clone(),
        });
    }
    Ok(())
}

/// The first field a read contract sets that only a capability changing state may set.
fn side_effect_field(contract: &CapabilityContract) -> Option<&'static str> {
    for field in [
        "reversibility",
        "idempotency",
        "verification",
        "constrainable_args",
        "safety_facts",
    ] {
        let set = match field {
            "reversibility" => contract.reversibility != Reversibility::NotApplicable,
            "idempotency" => contract.idempotency != Idempotency::Idempotent,
            "verification" => contract.verification != VerificationHint::None,
            "constrainable_args" => !contract.constrainable_args.is_empty(),
            _ => !contract.safety_facts.is_empty(),
        };
        if set {
            return Some(field);
        }
    }
    None
}

/// Whether the contract comes from the skill family.
fn is_skill(contract: &CapabilityContract) -> bool {
    match &contract.source {
        ContractSource::Skill { .. } => true,
        ContractSource::BuiltIn => false,
    }
}

/// Ties the target kind to the argument schema, so a contract cannot name a target the schema
/// does not ask for.
///
/// A kind that needs a target (`Container`, `Unit`) must name an argument the schema declares,
/// declares as text, and requires: without it the run cannot say what it acts on. `Host` is the
/// host itself and needs no argument; `None` names no target at all, so it may not take one
/// either. The two are not interchangeable: only `Host` binds an identity in a permit.
fn target_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    let needs_arg = matches!(
        contract.target_kind,
        TargetKind::Container | TargetKind::Unit
    );
    match (needs_arg, &contract.target_arg) {
        (false, Some(_)) | (true, None) => Err(ContractError::TargetArgMismatch {
            id: contract.id.clone(),
        }),
        (false, None) => Ok(()),
        (true, Some(key)) => check_target_arg(contract, key.as_str()),
    }
}

/// Checks one target argument against the schema that carries it.
fn check_target_arg(contract: &CapabilityContract, key: &str) -> Result<(), ContractError> {
    if !valid_arg_key(key) {
        return Err(ContractError::TargetArgBadName {
            id: contract.id.clone(),
            key: key.to_owned(),
        });
    }
    if !declares_argument(&contract.input_schema, key) {
        return Err(ContractError::TargetArgUndeclared {
            id: contract.id.clone(),
            key: key.to_owned(),
        });
    }
    match declared_type(&contract.input_schema, key) {
        Some(type_name) if type_name == "string" => {}
        _ => {
            return Err(ContractError::TargetArgNotText {
                id: contract.id.clone(),
                key: key.to_owned(),
            });
        }
    }
    if !requires_argument(&contract.input_schema, key) {
        return Err(ContractError::TargetArgNotRequired {
            id: contract.id.clone(),
            key: key.to_owned(),
        });
    }
    Ok(())
}

/// Checks every argument a host ceiling may constrain against the schema that carries it.
fn constraint_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    let mut seen = BTreeSet::<String>::new();
    for arg in &contract.constrainable_args {
        if !valid_arg_key(&arg.key) {
            return Err(ContractError::BadConstrainableKey {
                id: contract.id.clone(),
                key: arg.key.clone(),
            });
        }
        if !seen.insert(arg.key.clone()) {
            return Err(ContractError::DuplicateConstrainableArg {
                id: contract.id.clone(),
                key: arg.key.clone(),
            });
        }
        if !declares_argument(&contract.input_schema, &arg.key) {
            return Err(ContractError::ConstrainableUndeclared {
                id: contract.id.clone(),
                key: arg.key.clone(),
            });
        }
        match declared_type(&contract.input_schema, &arg.key) {
            Some(type_name) if type_name == "integer" => {}
            _ => {
                return Err(ContractError::ConstrainableNotInteger {
                    id: contract.id.clone(),
                    key: arg.key.clone(),
                });
            }
        }
        let (low, high) = stated_bounds(&contract.input_schema, &arg.key);
        if low.is_none() && high.is_none() {
            return Err(ContractError::ConstrainableUnbounded {
                id: contract.id.clone(),
                key: arg.key.clone(),
            });
        }
        if arg.description.trim().is_empty() {
            return Err(ContractError::ConstrainableBlankDescription {
                id: contract.id.clone(),
                key: arg.key.clone(),
            });
        }
    }
    Ok(())
}

/// Refuses a safety fact the evaluator would wait for twice.
fn fact_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    let mut seen = BTreeSet::<FactId>::new();
    for fact in &contract.safety_facts {
        if !seen.insert(fact.clone()) {
            return Err(ContractError::DuplicateSafetyFact {
                id: contract.id.clone(),
                fact: fact.clone(),
            });
        }
    }
    Ok(())
}

/// Checks what §9.2 asks of a containment, once the argument schema is known to be well formed.
///
/// A containment holds a host down on its own, so it must name the TTL it expires on, decide that
/// expiry outside the main agent, arm a watchdog of its own when it affects connectivity, and
/// journal both its apply and its undo. A `contain` contract that cannot say all of it is refused
/// here rather than accepted because it runs through the helper, and containment metadata on any
/// other class would read as authoritative for a containment that is never installed. `CeilingEntry`'s
/// `max_ttl` is a stricter-only agent-side limit and is never read as the containment's TTL.
fn containment_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    match &contract.containment {
        None if contract.effect == EffectClass::Contain => {
            Err(ContractError::ContainmentUndeclared {
                id: contract.id.clone(),
            })
        }
        None => Ok(()),
        Some(_) if contract.effect != EffectClass::Contain => {
            Err(ContractError::ContainmentNotApplicable {
                id: contract.id.clone(),
            })
        }
        Some(spec) => {
            check_containment_argument(contract, &spec.ttl_argument)?;
            if spec.expiry != ExpiryOwner::KernelTimer {
                return Err(ContractError::ContainmentExpiryNotIndependent {
                    id: contract.id.clone(),
                });
            }
            if spec.rollback != RollbackDeclaration::Journaled {
                return Err(ContractError::ContainmentRollbackNotJournalled {
                    id: contract.id.clone(),
                });
            }
            if let Some(key) = &spec.watchdog_argument {
                check_containment_argument(contract, key)?;
            }
            Ok(())
        }
    }
}

/// Checks one containment argument against the schema that carries it: a required integer with a
/// range of at least one second up to a stated bound, so the containment always carries a TTL that
/// can be compared with what the host allows and can never mean "hold forever".
fn check_containment_argument(
    contract: &CapabilityContract,
    key: &str,
) -> Result<(), ContractError> {
    if !valid_arg_key(key) {
        return Err(ContractError::ContainmentArgumentBadName {
            id: contract.id.clone(),
            key: key.to_owned(),
        });
    }
    if !declares_argument(&contract.input_schema, key) {
        return Err(ContractError::ContainmentArgumentUndeclared {
            id: contract.id.clone(),
            key: key.to_owned(),
        });
    }
    match declared_type(&contract.input_schema, key) {
        Some(type_name) if type_name == "integer" => {}
        _ => {
            return Err(ContractError::ContainmentArgumentNotInteger {
                id: contract.id.clone(),
                key: key.to_owned(),
            });
        }
    }
    if !requires_argument(&contract.input_schema, key) {
        return Err(ContractError::ContainmentArgumentNotRequired {
            id: contract.id.clone(),
            key: key.to_owned(),
        });
    }
    let (low, high) = stated_bounds(&contract.input_schema, key);
    match (low, high) {
        (Some(low), Some(high)) if low >= 1 && high >= low => {}
        _ => {
            return Err(ContractError::ContainmentRangeUnbounded {
                id: contract.id.clone(),
                key: key.to_owned(),
            });
        }
    }
    Ok(())
}

/// Refuses a recheck the hub would wait forever for or wait no time at all, and a postcondition
/// that says nothing.
fn verification_rules(contract: &CapabilityContract) -> Result<(), ContractError> {
    match &contract.verification {
        VerificationHint::Recheck { delay } if recheck_delay_out_of_range(delay) => {
            Err(ContractError::RecheckDelayInvalid {
                id: contract.id.clone(),
            })
        }
        VerificationHint::Postcondition { description } if description.trim().is_empty() => {
            Err(ContractError::BlankPostcondition {
                id: contract.id.clone(),
            })
        }
        VerificationHint::Recheck { .. }
        | VerificationHint::None
        | VerificationHint::Postcondition { .. } => Ok(()),
    }
}

/// Whether a recheck delay is negative or longer than an hour.
fn recheck_delay_out_of_range(delay: &SignedDuration) -> bool {
    *delay < SignedDuration::ZERO || *delay > MAX_RECHECK_DELAY
}

/// Whether `key` matches the argument grammar `[a-z][a-z0-9_]{0,63}`.
fn valid_arg_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    bytes.len() <= 64
        && first.is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}
