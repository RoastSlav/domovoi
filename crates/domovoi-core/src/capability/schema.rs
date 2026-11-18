//! The shape a contract's model-facing argument schema must have, and the cross-checks
//! other rules read it through.
//!
//! This is a small structural validator, not a JSON Schema implementation: it accepts the
//! closed vocabulary every Domovoi argument schema uses (`type`, `properties`, `required`,
//! `additionalProperties`, and per argument `type`, `pattern`, `minLength`, `maxLength`,
//! `minimum`, `maximum`, `enum`, `default`, `description`) and refuses anything else, so a
//! schema can never promise an argument the execution boundary will reject. Schemas here are
//! flat: an argument is a string, an integer or a boolean, never an object or an array, which
//! is what the read tools and the skill runner carry.
//!
//! Anchoring a `pattern` is not checked here. Read tools match a pattern unanchored (the disk
//! device is `^/dev/`), while a skill's own parameter rules require `^…$`; that difference
//! belongs to the skill validator, not to the shape of a schema.

use std::collections::BTreeSet;

use serde_json::Value;

use super::schema_argument::{check_argument, stated_int};

/// The longest an argument name may be, in bytes, matching what the read tools and the
/// skill runner accept at the call boundary.
const MAX_ARGUMENT_NAME: usize = 64;

/// One problem with a contract's argument schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    /// The schema is not a JSON object.
    #[error("the schema is not an object")]
    NotObject,
    /// The schema does not declare its arguments as an object's properties.
    #[error("the schema must declare `type` as `object`")]
    RootTypeNotObject,
    /// The schema has a key outside the closed root vocabulary.
    #[error("the schema has an unexpected key `{key}`")]
    RootKey {
        /// The rejected key.
        key: String,
    },
    /// The schema does not close unknown arguments with `additionalProperties: false`.
    #[error("the schema must set `additionalProperties` to `false`")]
    OpenAdditionalProperties,
    /// The schema has no `properties` object.
    #[error("the schema must declare `properties` as an object")]
    PropertiesNotObject,
    /// The schema has no `required` array.
    #[error("the schema must declare `required` as an array")]
    RequiredNotArray,
    /// An argument name is outside the grammar an agent may call it by.
    #[error("`{name}` is not an argument name an agent may call")]
    ArgumentName {
        /// The rejected name.
        name: String,
    },
    /// An argument is not one of the three types the boundary carries.
    #[error("argument `{name}` must be `string`, `integer` or `boolean`")]
    ArgumentTypeNotTransportable {
        /// The argument.
        name: String,
    },
    /// An argument is not described by an object.
    #[error("argument `{name}` must be described by an object")]
    PropertyNotObject {
        /// The argument.
        name: String,
    },
    /// An argument uses a key outside the closed vocabulary.
    #[error("argument `{name}` has an unexpected key `{key}`")]
    PropertyKeyUnknown {
        /// The argument.
        name: String,
        /// The rejected key.
        key: String,
    },
    /// An argument states a constraint its type cannot carry, so the boundary ignores it or
    /// rejects every value.
    #[error("argument `{name}` states `{key}`, which does not apply to its type")]
    ConstraintForType {
        /// The argument.
        name: String,
        /// The rejected key.
        key: String,
    },
    /// A bound or length is not a number at all, so the boundary could not compare it.
    #[error("argument `{name}` has a bound that is not a whole number")]
    BoundNotWhole {
        /// The argument.
        name: String,
    },
    /// A character length is below zero, which no text has.
    #[error("argument `{name}` needs a length of at least zero")]
    LengthNegative {
        /// The argument.
        name: String,
    },
    /// An integer argument states its bounds the wrong way round.
    #[error("argument `{name}` has `minimum` above `maximum`")]
    InvertedRange {
        /// The argument.
        name: String,
    },
    /// A text argument states its lengths the wrong way round.
    #[error("argument `{name}` has `minLength` above `maxLength`")]
    InvertedLength {
        /// The argument.
        name: String,
    },
    /// An enumeration is empty, repeats a value, holds blank text or is not text.
    #[error("argument `{name}` must enumerate at least one distinct non-blank text")]
    BadEnum {
        /// The argument.
        name: String,
    },
    /// An argument that is not text enumerates values, so the boundary has no type to carry them.
    #[error("argument `{name}` may only enumerate text")]
    EnumNotTextType {
        /// The argument.
        name: String,
    },
    /// A pattern is blank, so the schema describes no text the boundary can match.
    #[error("argument `{name}` needs a pattern that is not blank")]
    BlankPattern {
        /// The argument.
        name: String,
    },
    /// A description is blank, so the inventory and the approval card say nothing.
    #[error("argument `{name}` needs a description that is not blank")]
    BlankDescription {
        /// The argument.
        name: String,
    },
    /// The required list names an argument the schema does not declare.
    #[error("`required` names `{name}`, which the schema does not declare")]
    UnknownRequired {
        /// The undeclared name.
        name: String,
    },
    /// The required list repeats an argument.
    #[error("`required` names `{name}` twice")]
    DuplicateRequired {
        /// The repeated name.
        name: String,
    },
    /// An entry of the required list is not text.
    #[error("`required` must list argument names as text")]
    RequiredNotText,
    /// A default is not of the argument's type, so the boundary would apply a value it
    /// rejects.
    #[error("argument `{name}` has a default that is not of its type")]
    DefaultTypeMismatch {
        /// The argument.
        name: String,
    },
    /// A default is outside the bounds or lengths the schema states.
    #[error("argument `{name}` has a default outside its bounds")]
    DefaultOutsideBounds {
        /// The argument.
        name: String,
    },
    /// A default is not one of the enumerated values.
    #[error("argument `{name}` has a default outside its enumeration")]
    DefaultOutsideEnum {
        /// The argument.
        name: String,
    },
}

/// Checks the structure of one contract's argument schema.
pub fn validate_schema(schema: &Value) -> Result<(), SchemaError> {
    let Some(root) = schema.as_object() else {
        return Err(SchemaError::NotObject);
    };
    for (key, _) in root {
        if !root_key_is_known(key.as_str()) {
            return Err(SchemaError::RootKey {
                key: key.to_owned(),
            });
        }
    }
    if root.get("type").and_then(|value| value.as_str()) != Some("object") {
        return Err(SchemaError::RootTypeNotObject);
    }
    if root
        .get("additionalProperties")
        .and_then(|value| value.as_bool())
        != Some(false)
    {
        return Err(SchemaError::OpenAdditionalProperties);
    }

    let Some(properties) = root.get("properties") else {
        return Err(SchemaError::PropertiesNotObject);
    };
    let Some(declared) = properties.as_object() else {
        return Err(SchemaError::PropertiesNotObject);
    };
    for (name, definition) in declared {
        if !valid_argument_name(name.as_str()) {
            return Err(SchemaError::ArgumentName {
                name: name.to_owned(),
            });
        }
        if let Some(reason) = check_argument(name.as_str(), definition) {
            return Err(reason);
        }
    }

    let Some(required) = root.get("required") else {
        return Err(SchemaError::RequiredNotArray);
    };
    let Some(list) = required.as_array() else {
        return Err(SchemaError::RequiredNotArray);
    };
    let mut seen = BTreeSet::<String>::new();
    for entry in list {
        let Some(name) = entry.as_str() else {
            return Err(SchemaError::RequiredNotText);
        };
        if properties.get(name).is_none() {
            return Err(SchemaError::UnknownRequired {
                name: name.to_owned(),
            });
        }
        if !seen.insert(name.to_owned()) {
            return Err(SchemaError::DuplicateRequired {
                name: name.to_owned(),
            });
        }
    }
    Ok(())
}

/// Whether a schema key is in the closed root vocabulary.
fn root_key_is_known(key: &str) -> bool {
    matches!(
        key,
        "type" | "properties" | "required" | "additionalProperties"
    )
}

/// Whether `name` is an argument name an agent may call: `[a-z][a-z0-9_]{0,63}`.
fn valid_argument_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    bytes.len() <= MAX_ARGUMENT_NAME
        && first.is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// Whether the schema declares `name` as one of its arguments.
pub fn declares_argument(schema: &Value, name: &str) -> bool {
    let Some(properties) = schema.get("properties") else {
        return false;
    };
    properties.get(name).is_some()
}

/// The type the schema declares for argument `name`.
pub fn declared_type(schema: &Value, name: &str) -> Option<String> {
    let properties = schema.get("properties")?;
    let definition = properties.get(name)?;
    definition
        .get("type")
        .and_then(|value| value.as_str())
        .map(|text| text.to_owned())
}

/// The inclusive integer bounds the schema states for `name`, either side absent.
pub fn stated_bounds(schema: &Value, name: &str) -> (Option<i64>, Option<i64>) {
    let Some(properties) = schema.get("properties") else {
        return (None, None);
    };
    let Some(definition) = properties.get(name) else {
        return (None, None);
    };
    (
        stated_int(definition, "minimum"),
        stated_int(definition, "maximum"),
    )
}

/// Whether the schema requires `name`, so the capability cannot run without it.
pub fn requires_argument(schema: &Value, name: &str) -> bool {
    let Some(list) = schema.get("required").and_then(|value| value.as_array()) else {
        return false;
    };
    list.iter().any(|entry| entry.as_str() == Some(name))
}
