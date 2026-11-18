//! The data-defined `skill.<name>` capability family and the hub's raise-only floor.

use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::builtin::READ_OUTPUT_LIMIT;
use super::contract::{CapabilityContract, ContractError, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion};
use crate::skill::{ParamKind, ParamSpec, SkillName, SkillSummary, check_kind};
use crate::triage::MAX_SKILL_ARGS;

/// A hub-set minimum effect class for one skill.
///
/// Only `Read`, `Mutate` and `Destructive` are valid: `Contain` is reserved for typed helper
/// capabilities, so no skill may be `contain` and no floor may ask for it. A floor can raise a
/// skill's class, never lower it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "EffectClass", into = "EffectClass")]
pub struct SkillFloor(EffectClass);

impl SkillFloor {
    /// Validates `class` as a skill floor.
    pub fn new(class: EffectClass) -> Result<Self, FloorError> {
        if class == EffectClass::Contain {
            return Err(FloorError::ContainNotAllowed);
        }
        Ok(Self(class))
    }

    /// The class this floor enforces.
    #[must_use]
    pub const fn class(&self) -> EffectClass {
        self.0
    }
}

impl TryFrom<EffectClass> for SkillFloor {
    type Error = FloorError;

    fn try_from(class: EffectClass) -> Result<Self, Self::Error> {
        Self::new(class)
    }
}

impl From<SkillFloor> for EffectClass {
    fn from(floor: SkillFloor) -> Self {
        floor.0
    }
}

/// Why a class cannot be a skill floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FloorError {
    /// `Contain` belongs to typed helper capabilities, not to skills.
    #[error("a skill floor cannot be `contain`")]
    ContainNotAllowed,
}

/// Why a skill's data cannot become a contract.
///
/// The hub refuses a summary it cannot describe honestly rather than inventing a limit or a
/// verification procedure for it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillContractError {
    /// The timeout is outside 1 second to 1 hour, which is what the skill's manifest must say.
    #[error("the skill's timeout must be between 1s and 1h")]
    Timeout {
        /// The skill whose timeout was refused.
        skill: SkillName,
    },
    /// The skill declares more parameters than the skill-call boundary carries.
    #[error("declares {count} parameters, above the {limit} the call boundary carries")]
    TooManyParams {
        /// The skill.
        skill: SkillName,
        /// How many it declares.
        count: usize,
        /// The boundary's cap.
        limit: usize,
    },
    /// A parameter's kind is malformed, with the manifest's own reason.
    #[error("parameter `{param}`: {reason}")]
    ParamKind {
        /// The parameter.
        param: String,
        /// Why the kind was refused.
        reason: String,
    },
    /// A read skill declared `verify`, which would re-run a check nothing changed.
    #[error("a read skill cannot declare verify")]
    VerifyForRead {
        /// The skill.
        skill: SkillName,
    },
    /// A verify delay is outside 0 seconds to 1 hour.
    #[error("the verify delay must be between 0s and 1h")]
    VerifyDelay {
        /// The skill.
        skill: SkillName,
    },
    /// The contract the summary describes is refused by `validate`, which says the rule.
    #[error("the contract is refused: {reason}")]
    Invalid {
        /// The skill.
        skill: SkillName,
        /// The rule that refused it.
        reason: ContractError,
    },
}

/// The effective class of a skill: the stricter of its declared class and the hub's floor.
///
/// A floor can only raise; with no floor the declared class stands. An unknown declared
/// class has already become `Destructive` before it reaches here.
#[must_use]
pub fn effective_effect(declared: EffectClass, floor: Option<SkillFloor>) -> EffectClass {
    declared.max(floor.map_or(declared, |floor| floor.class()))
}

/// The contract of one skill as the hub sees it, or why the summary cannot be described.
///
/// The declared class comes from the skill's manifest and the effect is the stricter of that and
/// `floor`, so a provider can never lower its own class. The id is always `skill.<name>`, so a
/// skill can never claim a built-in id and a built-in can never take this one.
///
/// Every assumption the hub makes is checked, not assumed: the timeout must be the manifest's
/// range (1s to 1h) rather than clamped, the parameter kinds must be the ones the skill validator
/// accepts, the parameter count must fit the skill-call boundary, and a read skill must declare
/// no verify. The result has passed [`validate`](Self::validate), so a caller can hand it
/// straight to the registry.
///
/// Verification is never invented. `verify.recheck` with its delay becomes
/// [`VerificationHint::Recheck`], which keeps the delay the owner declared; a skill that declares
/// no verify, or one that says `recheck = false`, keeps [`VerificationHint::None`], which reads as
/// "the owner declared nothing" and is not the same as a procedure that failed. A skill's
/// category and effect class never say `contain`: containment belongs to typed helper
/// capabilities.
pub fn skill_contract(
    summary: &SkillSummary,
    floor: Option<SkillFloor>,
) -> Result<CapabilityContract, SkillContractError> {
    if summary.timeout < MIN_TIMEOUT || summary.timeout > MAX_TIMEOUT {
        return Err(SkillContractError::Timeout {
            skill: summary.name.clone(),
        });
    }
    if summary.params.len() > MAX_SKILL_ARGS {
        return Err(SkillContractError::TooManyParams {
            skill: summary.name.clone(),
            count: summary.params.len(),
            limit: MAX_SKILL_ARGS,
        });
    }
    for param in &summary.params {
        if let Some(reason) = check_kind(&param.kind).err() {
            return Err(SkillContractError::ParamKind {
                param: param.name.to_string(),
                reason,
            });
        }
    }
    let declared = EffectClass::from(summary.risk);
    if let Some(verify) = &summary.verify {
        if declared == EffectClass::Read {
            return Err(SkillContractError::VerifyForRead {
                skill: summary.name.clone(),
            });
        }
        if verify.delay < MIN_VERIFY_DELAY || verify.delay > MAX_VERIFY_DELAY {
            return Err(SkillContractError::VerifyDelay {
                skill: summary.name.clone(),
            });
        }
    }
    let effect = effective_effect(declared, floor);
    // No skill may be `contain`, so it has no skill category: the class belongs to typed
    // helper capabilities only.
    let category = match ContractCategory::skill(effect) {
        None => {
            return Err(SkillContractError::Invalid {
                skill: summary.name.clone(),
                reason: ContractError::SkillContain {
                    id: ContractId::skill(&summary.name),
                },
            });
        }
        Some(category) => category,
    };

    let contract = CapabilityContract {
        id: ContractId::skill(&summary.name),
        version: ContractVersion::first(),
        title: summary.name.to_string(),
        description: summary.description.clone(),
        input_schema: skill_input_schema(&summary.params),
        effect,
        category,
        target_kind: TargetKind::Host,
        target_arg: None,
        reversibility: match effect {
            EffectClass::Read => Reversibility::NotApplicable,
            EffectClass::Mutate => Reversibility::Reversible,
            // A destructive skill's own manifest may declare a `revert`, which the hub does not
            // see in the summary, so the class says the worst case: not undone.
            EffectClass::Contain | EffectClass::Destructive => Reversibility::Irreversible,
        },
        idempotency: if effect == EffectClass::Read {
            Idempotency::Idempotent
        } else {
            Idempotency::NotIdempotent
        },
        timeout: summary.timeout,
        output_limit_bytes: if effect == EffectClass::Read {
            READ_OUTPUT_LIMIT
        } else {
            SKILL_OUTPUT_LIMIT
        },
        verification: match &summary.verify {
            None => VerificationHint::None,
            Some(verify) if verify.recheck => VerificationHint::Recheck {
                delay: verify.delay,
            },
            Some(_) => VerificationHint::None,
        },
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution: ExecutionPath::SkillRunner,
        source: ContractSource::Skill {
            declared,
            floor: floor.map(|floor| floor.class()),
        },
    };
    contract
        .validate()
        .map_err(|reason| SkillContractError::Invalid {
            skill: summary.name.clone(),
            reason,
        })?;
    Ok(contract)
}

/// The output limit for a skill that is not a read, in bytes.
const SKILL_OUTPUT_LIMIT: u32 = 65_536;
/// The shortest and longest timeout a skill contract may carry, the manifest's range.
const MIN_TIMEOUT: SignedDuration = SignedDuration::from_secs(1);
const MAX_TIMEOUT: SignedDuration = SignedDuration::from_hours(1);
/// The range a declared verify delay may take.
const MIN_VERIFY_DELAY: SignedDuration = SignedDuration::ZERO;
const MAX_VERIFY_DELAY: SignedDuration = SignedDuration::from_hours(1);
/// The text an integer parameter's value is carried in, which is what the skill-call boundary
/// parses and normalises to canonical decimal form.
const INTEGER_TEXT: &str = r"^-?[0-9]+$";
/// The only two texts a boolean parameter's value is carried as.
const BOOLEAN_TEXT: &str = r"^(true|false)$";

/// The model-facing schema of a skill's declared parameters.
///
/// The skill-call boundary carries every value as text, so the schema states the text the
/// boundary accepts and not only what it means: an integer and a boolean name the pattern their
/// text must match ([`INTEGER_TEXT`] and [`BOOLEAN_TEXT`]), and the boundary's own rules
/// ([`SkillSummary::validate_args`] in the skill validator) parse an integer as a signed whole
/// number and accept only the two boolean words. Nothing here advertises an argument the
/// boundary would reject: unknown arguments are closed off, and `skill_contract` refuses more
/// parameters than the boundary carries.
#[must_use]
pub fn skill_input_schema(params: &[ParamSpec]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for param in params {
        properties.insert(param.name.to_string(), param_schema(param));
        if param.required {
            required.push(Value::String(param.name.to_string()));
        }
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// The schema of one declared parameter: its kind, the text the boundary carries, its bounds and
/// its description.
fn param_schema(param: &ParamSpec) -> Value {
    let mut object = Map::new();
    match &param.kind {
        ParamKind::String { pattern, max_len } => {
            object.insert("type".to_owned(), json!("string"));
            object.insert("pattern".to_owned(), json!(pattern));
            object.insert("maxLength".to_owned(), json!(max_len));
        }
        ParamKind::Integer { min, max } => {
            object.insert("type".to_owned(), json!("integer"));
            object.insert("pattern".to_owned(), json!(INTEGER_TEXT));
            object.insert("minimum".to_owned(), json!(min));
            object.insert("maximum".to_owned(), json!(max));
        }
        ParamKind::Boolean => {
            object.insert("type".to_owned(), json!("boolean"));
            object.insert("pattern".to_owned(), json!(BOOLEAN_TEXT));
        }
        ParamKind::Enum { values } => {
            object.insert("type".to_owned(), json!("string"));
            object.insert("enum".to_owned(), json!(values));
        }
    }
    object.insert("description".to_owned(), json!(param.description));
    Value::Object(object)
}
