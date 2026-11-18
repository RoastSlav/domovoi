//! The data-defined `skill.<name>` capability family and the hub's raise-only floor.

use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::builtin::READ_OUTPUT_LIMIT;
use super::contract::{CapabilityContract, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion};
use crate::skill::{ParamKind, ParamSpec, SkillSummary};

/// The output limit for a skill that is not a read, in bytes.
const SKILL_OUTPUT_LIMIT: u32 = 65_536;
/// The longest a skill contract may run.
const MAX_TIMEOUT: SignedDuration = SignedDuration::from_secs(3600);

/// A hub-set minimum effect class for one skill.
///
/// Only `Read`, `Mutate` and `Destructive` are valid: `Contain` is reserved for typed helper
/// capabilities. A floor can raise a skill's class, never lower it.
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

/// The effective class of a skill: the stricter of its declared class and the hub's floor.
///
/// A floor can only raise; with no floor the declared class stands. An unknown declared
/// class has already become `Destructive` before it reaches here.
#[must_use]
pub fn effective_effect(declared: EffectClass, floor: Option<SkillFloor>) -> EffectClass {
    declared.max(floor.map_or(declared, |floor| floor.class()))
}

/// The contract of one skill as the hub sees it.
///
/// The declared class comes from the skill's manifest; the effect is the stricter of that
/// and `floor`. The id is always `skill.<name>`, so a skill can never claim a built-in id.
#[must_use]
pub fn skill_contract(summary: &SkillSummary, floor: Option<SkillFloor>) -> CapabilityContract {
    let declared = EffectClass::from(summary.risk);
    let effect = effective_effect(declared, floor);
    let category = ContractCategory::from_static(match effect {
        EffectClass::Read => "skill.read",
        EffectClass::Mutate => "skill.mutate",
        // A skill's declared class is never `Contain`, so the effective class is never
        // `Contain` either; mapping it here fails closed rather than panicking.
        EffectClass::Contain | EffectClass::Destructive => "skill.destructive",
    });
    let reversibility = match effect {
        EffectClass::Read => Reversibility::NotApplicable,
        EffectClass::Mutate | EffectClass::Contain => Reversibility::Reversible,
        EffectClass::Destructive if summary.verify.is_some() => Reversibility::Reversible,
        EffectClass::Destructive => Reversibility::Irreversible,
    };
    let verification = match effect {
        EffectClass::Read => VerificationHint::None,
        _ => match &summary.verify {
            Some(verify) if verify.recheck => VerificationHint::Recheck,
            _ => VerificationHint::Postcondition {
                description: "none declared by the skill".to_owned(),
            },
        },
    };
    // Upper clamp only: a zero or negative timeout is left exactly as declared, so the
    // contract fails `validate` rather than being silently repaired here.
    let timeout = if summary.timeout > MAX_TIMEOUT {
        MAX_TIMEOUT
    } else {
        summary.timeout
    };

    CapabilityContract {
        id: ContractId::skill(&summary.name),
        version: ContractVersion::first(),
        title: summary.name.to_string(),
        description: summary.description.clone(),
        input_schema: skill_input_schema(&summary.params),
        effect,
        category,
        target_kind: TargetKind::Host,
        target_arg: None,
        reversibility,
        idempotency: if effect == EffectClass::Read {
            Idempotency::Idempotent
        } else {
            Idempotency::NotIdempotent
        },
        timeout,
        output_limit_bytes: if effect == EffectClass::Read {
            READ_OUTPUT_LIMIT
        } else {
            SKILL_OUTPUT_LIMIT
        },
        verification,
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution: ExecutionPath::SkillRunner,
        source: ContractSource::Skill {
            declared,
            floor: floor.map(|floor| floor.class()),
        },
    }
}

/// The JSON Schema of a skill's declared parameters.
///
/// The schema describes what each argument means; the wire may still carry every argument
/// as a string.
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

/// The schema of one declared parameter, with its description.
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
            object.insert("minimum".to_owned(), json!(min));
            object.insert("maximum".to_owned(), json!(max));
        }
        ParamKind::Boolean => {
            object.insert("type".to_owned(), json!("boolean"));
        }
        ParamKind::Enum { values } => {
            object.insert("type".to_owned(), json!("string"));
            object.insert("enum".to_owned(), json!(values));
        }
    }
    object.insert("description".to_owned(), json!(param.description));
    Value::Object(object)
}
