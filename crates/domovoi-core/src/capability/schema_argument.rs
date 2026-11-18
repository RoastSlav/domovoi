//! One argument's definition inside a contract's schema: its keys, its type and the
//! coherence of its constraints. These are the per-argument half of the checks
//! [`validate_schema`](../schema) runs over the whole schema.

use std::collections::BTreeSet;

use serde_json::Value;

use super::schema::SchemaError;

/// Whether a schema key is in the closed vocabulary one argument may use.
pub fn argument_key_is_known(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "pattern"
            | "minLength"
            | "maxLength"
            | "minimum"
            | "maximum"
            | "enum"
            | "default"
            | "description"
    )
}

/// Checks one argument's definition: its keys, its type and the coherence of its constraints.
pub fn check_argument(name: &str, definition: &Value) -> Option<SchemaError> {
    let Some(fields) = definition.as_object() else {
        return Some(SchemaError::PropertyNotObject {
            name: name.to_owned(),
        });
    };
    for (key, _) in fields {
        if !argument_key_is_known(key.as_str()) {
            return Some(SchemaError::PropertyKeyUnknown {
                name: name.to_owned(),
                key: key.to_owned(),
            });
        }
    }

    let kind = definition.get("type").and_then(|value| value.as_str());
    if !matches!(kind, Some("string") | Some("integer") | Some("boolean")) {
        return Some(SchemaError::ArgumentTypeNotTransportable {
            name: name.to_owned(),
        });
    }

    for key in ["pattern", "minLength", "maxLength", "minimum", "maximum"] {
        if definition.get(key).is_some() && !constraint_applies(kind, key) {
            return Some(SchemaError::ConstraintForType {
                name: name.to_owned(),
                key: key.to_owned(),
            });
        }
    }

    for key in ["minimum", "maximum"] {
        if definition.get(key).is_some() && stated_int(definition, key).is_none() {
            return Some(SchemaError::BoundNotWhole {
                name: name.to_owned(),
            });
        }
    }
    for key in ["minLength", "maxLength"] {
        match stated_int(definition, key) {
            None => {}
            Some(value) if value < 0 => {
                return Some(SchemaError::LengthNegative {
                    name: name.to_owned(),
                });
            }
            Some(_) => {}
        }
    }
    match (
        stated_int(definition, "minimum"),
        stated_int(definition, "maximum"),
    ) {
        (Some(low), Some(high)) if low > high => {
            return Some(SchemaError::InvertedRange {
                name: name.to_owned(),
            });
        }
        _ => {}
    }
    match (
        stated_int(definition, "minLength"),
        stated_int(definition, "maxLength"),
    ) {
        (Some(low), Some(high)) if low > high => {
            return Some(SchemaError::InvertedLength {
                name: name.to_owned(),
            });
        }
        _ => {}
    }

    if let Some(pattern) = definition.get("pattern")
        && pattern.as_str().is_none_or(|text| text.trim().is_empty())
    {
        return Some(SchemaError::BlankPattern {
            name: name.to_owned(),
        });
    }
    if let Some(description) = definition.get("description")
        && description
            .as_str()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Some(SchemaError::BlankDescription {
            name: name.to_owned(),
        });
    }

    if definition.get("enum").is_some() {
        if !matches!(kind, Some("string")) {
            return Some(SchemaError::EnumNotTextType {
                name: name.to_owned(),
            });
        }
        let Some(values) = definition.get("enum").and_then(|value| value.as_array()) else {
            return Some(SchemaError::BadEnum {
                name: name.to_owned(),
            });
        };
        if values.is_empty() {
            return Some(SchemaError::BadEnum {
                name: name.to_owned(),
            });
        }
        let mut seen = BTreeSet::<String>::new();
        for value in values {
            let Some(text) = value.as_str() else {
                return Some(SchemaError::BadEnum {
                    name: name.to_owned(),
                });
            };
            if text.trim().is_empty() || !seen.insert(text.to_owned()) {
                return Some(SchemaError::BadEnum {
                    name: name.to_owned(),
                });
            }
        }
    }

    if let Some(reason) = check_default(name, definition, kind) {
        return Some(reason);
    }
    None
}

/// Whether a constraint key fits the argument's type. `enum` is checked separately, so that a
/// better message can name it. `pattern` also fits integers and booleans: the call boundary
/// carries every value as text, so a `pattern` on them describes the text the boundary accepts.
pub fn constraint_applies(kind: Option<&str>, key: &str) -> bool {
    matches!(
        (kind, key),
        (Some("integer"), "minimum" | "maximum" | "pattern")
            | (Some("string"), "pattern" | "minLength" | "maxLength")
            | (Some("boolean"), "pattern")
    )
}

/// The whole number a definition states under `key`; `None` for a fraction, a string, a
/// boolean or an absent key. Bounds may be negative; lengths are checked separately.
pub fn stated_int(definition: &Value, key: &str) -> Option<i64> {
    definition.get(key).and_then(|value| value.as_i64())
}

/// Checks that an argument's default is one its own schema would accept.
pub fn check_default(name: &str, definition: &Value, kind: Option<&str>) -> Option<SchemaError> {
    let default = definition.get("default")?;
    match kind {
        Some("integer") => {
            let Some(number) = default.as_i64() else {
                return Some(SchemaError::DefaultTypeMismatch {
                    name: name.to_owned(),
                });
            };
            let low = stated_int(definition, "minimum");
            let high = stated_int(definition, "maximum");
            let outside = match (low, high) {
                (Some(low), _) if number < low => true,
                (_, Some(high)) if number > high => true,
                _ => false,
            };
            if outside {
                return Some(SchemaError::DefaultOutsideBounds {
                    name: name.to_owned(),
                });
            }
            None
        }
        Some("string") => {
            let Some(text) = default.as_str() else {
                return Some(SchemaError::DefaultTypeMismatch {
                    name: name.to_owned(),
                });
            };
            let length = text.chars().count() as i64;
            let low = stated_int(definition, "minLength");
            let high = stated_int(definition, "maxLength");
            let outside = match (low, high) {
                (Some(low), _) if length < low => true,
                (_, Some(high)) if length > high => true,
                _ => false,
            };
            if outside {
                return Some(SchemaError::DefaultOutsideBounds {
                    name: name.to_owned(),
                });
            }
            if let Some(values) = definition.get("enum").and_then(|value| value.as_array())
                && !values.iter().any(|value| value.as_str() == Some(text))
            {
                return Some(SchemaError::DefaultOutsideEnum {
                    name: name.to_owned(),
                });
            }
            None
        }
        Some("boolean") => {
            if default.as_bool().is_none() {
                return Some(SchemaError::DefaultTypeMismatch {
                    name: name.to_owned(),
                });
            }
            None
        }
        _ => None,
    }
}
