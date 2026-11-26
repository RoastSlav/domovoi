//! The typed description of one capability.

use std::collections::BTreeSet;

use jiff::SignedDuration;
use serde::Serialize;

use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion, FactId};
use super::skill::{SkillFloor, effective_effect};

/// The largest output a contract may promise, in bytes.
const MAX_OUTPUT_LIMIT: u32 = 1_048_576;
/// The longest a capability may run, as contracts state it.
const MAX_TIMEOUT: SignedDuration = SignedDuration::from_secs(3600);

/// The hub's own description of what one capability is and how dangerous it is.
///
/// Only the hub constructs these, from its built-in table or from a skill summary plus a
/// floor. There is no `Deserialize`, so nothing an agent or a skill author sends can build
/// one, and the effect class never comes from a wire value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityContract {
    /// Stable registry and policy id.
    pub id: ContractId,
    /// Version of this contract's shape and meaning.
    pub version: ContractVersion,
    /// Short owner-facing title for the capability inventory.
    pub title: String,
    /// One or two sentences saying what the capability does.
    pub description: String,
    /// JSON Schema of the capability's arguments.
    pub input_schema: serde_json::Value,
    /// What running the capability can do to a host.
    pub effect: EffectClass,
    /// The coarse family used for policy specificity.
    pub category: ContractCategory,
    /// What kind of thing the capability acts on.
    pub target_kind: TargetKind,
    /// The argument naming the target, when the target kind needs one.
    pub target_arg: Option<String>,
    /// Whether the effect can be undone.
    pub reversibility: Reversibility,
    /// Whether a repeated run changes anything further.
    pub idempotency: Idempotency,
    /// How long one run may take.
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub timeout: SignedDuration,
    /// The most output the hub keeps and shows, in bytes.
    pub output_limit_bytes: u32,
    /// How to check that the capability worked.
    pub verification: VerificationHint,
    /// The arguments a host ceiling may constrain (D-18).
    pub constrainable_args: Vec<ConstrainableArg>,
    /// Facts that must be known before the policy evaluator may allow it.
    pub safety_facts: Vec<FactId>,
    /// Where the capability executes (RW-39).
    pub execution: ExecutionPath,
    /// Where this contract comes from.
    pub source: ContractSource,
}

/// One argument an unattended host ceiling may constrain, and how its bound is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstrainableArg {
    /// The argument's key in the input schema.
    pub key: String,
    /// Whether the ceiling's bound is a minimum or a maximum.
    pub kind: ConstraintKind,
    /// What the bound means, for the approval card and the inventory.
    pub description: String,
}

/// How an integer ceiling bound on a constrainable argument is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    /// The argument must be at least the ceiling's value.
    IntAtLeast,
    /// The argument must be at most the ceiling's value.
    IntAtMost,
}

/// Where a contract comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContractSource {
    /// Typed in `domovoi-core`; the hub owns it entirely.
    BuiltIn,
    /// A skill. `declared` is what the manifest says and `floor` is the hub's raised
    /// minimum, if any; the contract's `effect` is the stricter of the two.
    Skill {
        /// The class the skill's manifest declares.
        declared: EffectClass,
        /// The hub's allowed minimum, when the owner raised it.
        floor: Option<EffectClass>,
    },
}

/// Why a contract is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    /// The output limit is outside the allowed range.
    #[error("{id}: output limit {bytes} is outside 1..=1048576 bytes")]
    OutputLimit {
        /// The contract that failed.
        id: ContractId,
        /// The rejected limit.
        bytes: u32,
    },
    /// The timeout is zero, negative or longer than an hour.
    #[error("{id}: timeout must be positive and at most one hour")]
    Timeout {
        /// The contract that failed.
        id: ContractId,
    },
    /// The target argument and the target kind disagree.
    #[error("{id}: the target argument and target kind disagree")]
    TargetArgMismatch {
        /// The contract that failed.
        id: ContractId,
    },
    /// A read contract set a field only a mutating contract may set.
    #[error("{id}: a read contract cannot set {field}")]
    ReadHasSideEffectFields {
        /// The contract that failed.
        id: ContractId,
        /// The field that does not apply to a read.
        field: &'static str,
    },
    /// A mutating contract did not say how it is undone and verified.
    #[error("{id}: a mutating contract needs a reversibility and a verification")]
    MutatingNeedsReversibilityAndVerification {
        /// The contract that failed.
        id: ContractId,
    },
    /// The id and the source disagree about whether this is a skill.
    #[error("{id}: a skill contract needs a `skill.` id, a skill source and the skill runner")]
    SkillSourceMismatch {
        /// The contract that failed.
        id: ContractId,
    },
    /// The contract's effect is not what its declared class and floor imply.
    #[error("{id}: expected effect {expected:?}, found {found:?}")]
    SkillEffectMismatch {
        /// The contract that failed.
        id: ContractId,
        /// The effect the source implies.
        expected: EffectClass,
        /// The effect the contract states.
        found: EffectClass,
    },
    /// A skill contract is, or claims to be, `contain`.
    #[error("{id}: a skill contract cannot be `contain`")]
    SkillContain {
        /// The contract that failed.
        id: ContractId,
    },
    /// A constrainable argument is declared twice.
    #[error("{id}: constrainable argument `{key}` is declared twice")]
    DuplicateConstrainableArg {
        /// The contract that failed.
        id: ContractId,
        /// The repeated key.
        key: String,
    },
    /// A constrainable argument key does not match the argument grammar.
    #[error("{id}: constrainable argument `{key}` must match [a-z][a-z0-9_]{{0,63}}")]
    BadConstrainableKey {
        /// The contract that failed.
        id: ContractId,
        /// The rejected key.
        key: String,
    },
    /// The input schema is not a JSON Schema object.
    #[error("{id}: the input schema must be an object with type `object`")]
    SchemaNotObject {
        /// The contract that failed.
        id: ContractId,
    },
}

impl CapabilityContract {
    /// Checks that every field is consistent with the others.
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

        let needs_target_arg = matches!(
            self.target_kind,
            TargetKind::Container | TargetKind::Unit | TargetKind::NetworkSource
        );
        if self.target_arg.is_some() != needs_target_arg {
            return Err(ContractError::TargetArgMismatch {
                id: self.id.clone(),
            });
        }

        if self.effect == EffectClass::Read {
            let side_effect_field = if self.reversibility != Reversibility::NotApplicable {
                Some("reversibility")
            } else if self.idempotency != Idempotency::Idempotent {
                Some("idempotency")
            } else if self.verification != VerificationHint::None {
                Some("verification")
            } else if !self.constrainable_args.is_empty() {
                Some("constrainable_args")
            } else if !self.safety_facts.is_empty() {
                Some("safety_facts")
            } else {
                None
            };
            if let Some(field) = side_effect_field {
                return Err(ContractError::ReadHasSideEffectFields {
                    id: self.id.clone(),
                    field,
                });
            }
        } else if self.reversibility == Reversibility::NotApplicable
            || self.verification == VerificationHint::None
        {
            return Err(ContractError::MutatingNeedsReversibilityAndVerification {
                id: self.id.clone(),
            });
        }

        match &self.source {
            ContractSource::BuiltIn => {
                if self.id.is_skill() || self.execution == ExecutionPath::SkillRunner {
                    return Err(ContractError::SkillSourceMismatch {
                        id: self.id.clone(),
                    });
                }
            }
            ContractSource::Skill { declared, floor } => {
                if !self.id.is_skill()
                    || self.execution != ExecutionPath::SkillRunner
                    || self.target_kind != TargetKind::Host
                {
                    return Err(ContractError::SkillSourceMismatch {
                        id: self.id.clone(),
                    });
                }
                if *declared == EffectClass::Contain
                    || self.effect == EffectClass::Contain
                    || floor == &Some(EffectClass::Contain)
                {
                    return Err(ContractError::SkillContain {
                        id: self.id.clone(),
                    });
                }
                let floor =
                    match floor {
                        Some(class) => Some(SkillFloor::new(*class).map_err(|_| {
                            ContractError::SkillContain {
                                id: self.id.clone(),
                            }
                        })?),
                        None => None,
                    };
                let expected = effective_effect(*declared, floor);
                if expected != self.effect {
                    return Err(ContractError::SkillEffectMismatch {
                        id: self.id.clone(),
                        expected,
                        found: self.effect,
                    });
                }
            }
        }

        let mut seen = BTreeSet::new();
        for arg in &self.constrainable_args {
            if !valid_arg_key(&arg.key) {
                return Err(ContractError::BadConstrainableKey {
                    id: self.id.clone(),
                    key: arg.key.clone(),
                });
            }
            if !seen.insert(&arg.key) {
                return Err(ContractError::DuplicateConstrainableArg {
                    id: self.id.clone(),
                    key: arg.key.clone(),
                });
            }
        }

        if !self.input_schema.is_object()
            || self.input_schema.get("type").and_then(|t| t.as_str()) != Some("object")
        {
            return Err(ContractError::SchemaNotObject {
                id: self.id.clone(),
            });
        }

        Ok(())
    }
}

/// Whether `key` matches `[a-z][a-z0-9_]{0,63}`.
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
