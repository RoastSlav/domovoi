//! Validated identifiers a capability contract is named by.
//!
//! Every value is an opaque string checked at parse time: a contract id or category is
//! dot-separated lowercase words, a target name is free text an owner typed, and a target
//! identity is the canonical `<scheme>:<rest>` string permits and plan hashes bind to.
//!
//! Namespaces are decided here, by grammar and not by a lookup table, so a later prompt can add
//! a built-in family without reopening this file:
//!
//! - a contract id is `<family>.<name>`; the family `skill` is reserved for the skill family, so
//!   every other first segment is a built-in family a later prompt may introduce.
//! - a skill's id is always `skill.<name>` ([`ContractId::skill`]) and never a built-in id; a
//!   built-in id never starts with `skill.` ([`ContractId::is_skill`]).
//! - a skill's category is exactly `skill.<effect class>` ([`ContractCategory::skill`]); a built-in
//!   category may not take that shape, so a rule naming a category cannot mix the two families.
//! - `TargetIdentity::parse` checks only the shape. Resolving an owner-typed name to an identity
//!   belongs to the target resolver, which is a separate boundary.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::effect::{EffectClass, TargetKind};
use crate::error::ValidationError;
use crate::skill::SkillName;

/// The longest contract id, in bytes.
const MAX_CONTRACT_ID: usize = 80;
/// The longest contract category, in bytes.
const MAX_CATEGORY: usize = 64;
/// The longest fact id, in bytes.
const MAX_FACT_ID: usize = 64;
/// The most characters a target name may hold.
const MAX_TARGET_NAME: usize = 128;
/// The most characters the rest of a target identity may hold.
const MAX_IDENTITY_REST: usize = 300;
/// The longest target identity, scheme and separator included.
const MAX_IDENTITY: usize = 310;

/// Defines an opaque string newtype with the shared parse, display and serde plumbing.
macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The text of this identifier.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

string_newtype!(
    /// A capability contract's stable id, for example `container.list` or `skill.backup_db`.
    ///
    /// One or more `.`-separated segments each matching `[a-z][a-z0-9_]*`, at least two
    /// segments, at most 80 bytes. The id names a capability in the registry and in policy
    /// rules; it is never a model-facing tool name.
    ContractId
);

string_newtype!(
    /// The coarse family a contract belongs to, for example `container.observe`.
    ///
    /// Same grammar as [`ContractId`], at most 64 bytes. Every category belongs to exactly
    /// one effect class, which the policy evaluator's specificity order depends on. A category
    /// in the reserved `skill.` namespace names a skill family and takes the shape `skill.`
    /// followed by the effect class; a built-in category never takes that shape, and a built-in
    /// read category ends in the segment `observe`.
    ContractCategory
);

string_newtype!(
    /// The name of a fact the policy evaluator may require to be known before allowing a
    /// capability, for example `host.not_in_maintenance`.
    ///
    /// `[a-z][a-z0-9_.]*`, at most 64 bytes; dots are allowed and a single segment is fine.
    FactId
);

string_newtype!(
    /// What an owner types in a policy rule to name a target, for example `plex`.
    ///
    /// Free text of 1 to 128 characters with no control characters and no leading or
    /// trailing whitespace. Matching is exact and case-sensitive.
    TargetName
);

string_newtype!(
    /// The canonical identity of a target, for example `container:2b0c.../plex-id`.
    ///
    /// The shape is `<scheme>:<rest>` where the scheme is `host`, `container`, `unit` or
    /// `ipnet` and the rest is 1 to 300 printable ASCII characters without spaces. Only the
    /// shape is checked here; resolving an owner-typed name to an identity is a separate
    /// boundary. `ipnet` is validated as a shape only: no capability here binds one, because
    /// the target kind and the resolver for network sources arrive later.
    TargetIdentity
);

impl ContractId {
    /// Builds an id from a literal the crate itself owns.
    ///
    /// Only the fixed built-in contract table uses this; every value from a provider or an
    /// owner goes through [`parse`](Self::parse).
    pub(crate) fn from_static(value: &'static str) -> Self {
        Self(value.to_owned())
    }

    /// Validates `input` as a contract id.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        check_dotted(input, "contract id", MAX_CONTRACT_ID, 2)?;
        Ok(Self(input.to_owned()))
    }

    /// The id of the skill family member `name`: `skill.<name>`.
    ///
    /// Cannot fail: a skill name already matches the segment grammar.
    #[must_use]
    pub fn skill(name: &SkillName) -> ContractId {
        Self(format!("skill.{}", name.as_str()))
    }

    /// The skill name when this id is a `skill.<name>` member.
    #[must_use]
    pub fn skill_name(&self) -> Option<SkillName> {
        self.0
            .strip_prefix("skill.")
            .and_then(|name| SkillName::parse(name).ok())
    }

    /// Whether this id belongs to the `skill.` family, the only namespace a skill may use.
    ///
    /// A built-in id never starts with `skill.`, so a skill can never name a built-in
    /// capability, and `validate` refuses the converse.
    #[must_use]
    pub fn is_skill(&self) -> bool {
        self.0.starts_with("skill.")
    }
}

impl ContractCategory {
    /// Builds a category from a literal the crate itself owns.
    ///
    /// Only the fixed built-in and skill tables use this; every value from a provider or
    /// an owner goes through [`parse`](Self::parse).
    pub(crate) fn from_static(value: &'static str) -> Self {
        Self(value.to_owned())
    }

    /// Validates `input` as a contract category.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        check_dotted(input, "contract category", MAX_CATEGORY, 2)?;
        Ok(Self(input.to_owned()))
    }

    /// Whether this category sits in the `skill.` namespace, which only the skill family may
    /// use. A built-in family takes any other first segment, so a later built-in family needs
    /// no change here.
    #[must_use]
    pub fn is_skill_namespace(&self) -> bool {
        self.0.starts_with("skill.")
    }

    /// The category of a skill capability whose effective class is `class`.
    ///
    /// `None` for `Contain`: no skill may be `contain`, so the category does not exist.
    #[must_use]
    pub fn skill(class: EffectClass) -> Option<ContractCategory> {
        if class == EffectClass::Contain {
            return None;
        }
        Some(Self(format!("skill.{}", class.as_str())))
    }

    /// The skill effect class this category names, when it is in the skill namespace.
    #[must_use]
    pub fn skill_class(&self) -> Option<EffectClass> {
        let name = self.0.strip_prefix("skill.")?;
        EffectClass::ALL
            .iter()
            .find(|class| class.as_str() == name)
            .copied()
    }

    /// Whether this category names *observing* a built-in family, the convention every
    /// built-in read contract follows: its last segment is `observe`.
    #[must_use]
    pub fn is_observation(&self) -> bool {
        self.0
            .rsplit(['.'])
            .next()
            .is_some_and(|last| last == "observe")
    }
}

impl FactId {
    /// Validates `input` as a fact id.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.is_empty() {
            return Err(ValidationError::new("fact id", "must not be empty"));
        }
        if input.len() > MAX_FACT_ID {
            return Err(ValidationError::new(
                "fact id",
                format!("must be at most {MAX_FACT_ID} characters"),
            ));
        }
        let bytes = input.as_bytes();
        if !bytes[0].is_ascii_lowercase() {
            return Err(ValidationError::new(
                "fact id",
                "must start with a lowercase letter",
            ));
        }
        if !bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'.')
        {
            return Err(ValidationError::new(
                "fact id",
                "may contain only a-z, 0-9, underscore and dot",
            ));
        }
        Ok(Self(input.to_owned()))
    }
}

impl TargetName {
    /// Validates `input` as a target name.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let mut chars = input.chars();
        let Some(first) = chars.next() else {
            return Err(ValidationError::new("target name", "must not be empty"));
        };
        if input.chars().count() > MAX_TARGET_NAME {
            return Err(ValidationError::new(
                "target name",
                format!("must be at most {MAX_TARGET_NAME} characters"),
            ));
        }
        if input.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "target name",
                "must not contain control characters",
            ));
        }
        if first.is_whitespace() || chars.next_back().is_some_and(char::is_whitespace) {
            return Err(ValidationError::new(
                "target name",
                "must not start or end with whitespace",
            ));
        }
        Ok(Self(input.to_owned()))
    }
}

impl TargetIdentity {
    /// Validates `input` as a target identity.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.len() > MAX_IDENTITY {
            return Err(ValidationError::new(
                "target identity",
                format!("must be at most {MAX_IDENTITY} characters"),
            ));
        }
        let Some((scheme, rest)) = input.split_once(':') else {
            return Err(ValidationError::new(
                "target identity",
                "must be `<scheme>:<rest>`",
            ));
        };
        if !matches!(scheme, "host" | "container" | "unit" | "ipnet") {
            return Err(ValidationError::new(
                "target identity",
                "scheme must be host, container, unit or ipnet",
            ));
        }
        if rest.is_empty() {
            return Err(ValidationError::new(
                "target identity",
                "the rest after the scheme must not be empty",
            ));
        }
        if rest.chars().count() > MAX_IDENTITY_REST {
            return Err(ValidationError::new(
                "target identity",
                format!("the rest after the scheme must be at most {MAX_IDENTITY_REST} characters"),
            ));
        }
        if !rest.chars().all(|c| c.is_ascii_graphic()) {
            return Err(ValidationError::new(
                "target identity",
                "the rest after the scheme must be printable ASCII without spaces",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The identity's scheme, which says which kind of thing it names.
    #[must_use]
    pub fn scheme(&self) -> String {
        split_identity(&self.0).0
    }

    /// The target kind this identity's scheme names, or `None` when no target kind exists for
    /// it yet.
    ///
    /// `ipnet` has none: a network source arrives with its own target kind and resolver, so
    /// nothing here may bind such an identity. Comparing a contract's target kind with an
    /// identity is not name resolution; the identity must already be canonical.
    #[must_use]
    pub fn target_kind(&self) -> Option<TargetKind> {
        match split_identity(&self.0).0.as_str() {
            "host" => Some(TargetKind::Host),
            "container" => Some(TargetKind::Container),
            "unit" => Some(TargetKind::Unit),
            // `ipnet`, and any scheme a value did not come through `parse`.
            _ => None,
        }
    }
}

/// A capability contract's version, starting at 1 and never 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct ContractVersion(u32);

impl ContractVersion {
    /// The version a contract starts at, which is always valid.
    pub(crate) const fn first() -> Self {
        Self(1)
    }

    /// Validates `value` as a contract version; 0 is refused.
    pub fn new(value: u32) -> Result<Self, ValidationError> {
        if value == 0 {
            return Err(ValidationError::new(
                "contract version",
                "must be at least 1",
            ));
        }
        Ok(Self(value))
    }

    /// The version as a number.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for ContractVersion {
    type Error = ValidationError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for ContractVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<ContractVersion> for u32 {
    fn from(value: ContractVersion) -> Self {
        value.0
    }
}

/// Splits an identity into scheme and rest, without validating it: an identity that did not
/// come through `TargetIdentity::parse` can be total here and still be meaningless.
fn split_identity(value: &str) -> (String, String) {
    match value.split_once(':') {
        Some((scheme, rest)) => (scheme.to_owned(), rest.to_owned()),
        None => (String::new(), value.to_owned()),
    }
}

/// Checks a dot-separated identifier with `min_segments` segments, each `[a-z][a-z0-9_]*`.
fn check_dotted(
    input: &str,
    field: &'static str,
    max: usize,
    min_segments: usize,
) -> Result<(), ValidationError> {
    if input.is_empty() {
        return Err(ValidationError::new(field, "must not be empty"));
    }
    if input.len() > max {
        return Err(ValidationError::new(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    let mut segments = 0usize;
    for segment in input.split('.') {
        check_segment(segment, field)?;
        segments += 1;
    }
    if segments < min_segments {
        return Err(ValidationError::new(
            field,
            format!("must have at least {min_segments} dot-separated segments"),
        ));
    }
    Ok(())
}

/// Checks one segment matches `[a-z][a-z0-9_]*`.
fn check_segment(segment: &str, field: &'static str) -> Result<(), ValidationError> {
    let bytes = segment.as_bytes();
    let Some(first) = bytes.first() else {
        return Err(ValidationError::new(field, "segments must not be empty"));
    };
    if !first.is_ascii_lowercase() {
        return Err(ValidationError::new(
            field,
            "each segment must start with a lowercase letter",
        ));
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
    {
        return Err(ValidationError::new(
            field,
            "segments may contain only a-z, 0-9 and underscore",
        ));
    }
    Ok(())
}
