//! One argument's definition inside a contract's schema: its keys, its type and the
//! coherence of its constraints. These are the per-argument half of the checks
//! [`validate_schema`](../schema) runs over the whole schema.
//!
//! A `pattern` is checked to compile but not to be anchored: read tools match it unanchored,
//! while the skill validator requires `^…$` and wraps every pattern it compiles; that
//! difference belongs to the boundary, not to the shape of a schema. A default is compared
//! against a pattern in the transport's own text form, because the boundary carries every
//! value as text: a string compares as itself, an integer as its canonical decimal form and
//! a boolean as `true` or `false`.

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

    let pattern = match definition.get("pattern") {
        None => None,
        Some(held) => {
            let Some(text) = held.as_str() else {
                return Some(SchemaError::PatternNotText {
                    name: name.to_owned(),
                });
            };
            if text.trim().is_empty() {
                return Some(SchemaError::BlankPattern {
                    name: name.to_owned(),
                });
            }
            match compile_pattern(text) {
                Ok(compiled) => Some(compiled),
                Err(reason) => {
                    return Some(SchemaError::InvalidPattern {
                        name: name.to_owned(),
                        reason,
                    });
                }
            }
        }
    };
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
            if let Some(reason) = enum_member_violates(name, definition, &pattern, text) {
                return Some(reason);
            }
        }
    }

    if let Some(reason) = check_default(name, definition, kind, &pattern) {
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

/// Checks that an argument's default is one its own schema would accept: the right type, inside
/// the bounds and lengths it states, one of its enumeration, and matching its pattern in the
/// transport's own text form, because the call boundary carries every value as text.
pub(crate) fn check_default(
    name: &str,
    definition: &Value,
    kind: Option<&str>,
    pattern: &Option<regex::Regex>,
) -> Option<SchemaError> {
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
            if !default_allowed(pattern, &number.to_string()) {
                return Some(SchemaError::DefaultOutsidePattern {
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
            if !default_allowed(pattern, text) {
                return Some(SchemaError::DefaultOutsidePattern {
                    name: name.to_owned(),
                });
            }
            None
        }
        Some("boolean") => {
            let Some(flag) = default.as_bool() else {
                return Some(SchemaError::DefaultTypeMismatch {
                    name: name.to_owned(),
                });
            };
            if !default_allowed(pattern, bool_text(flag)) {
                return Some(SchemaError::DefaultOutsidePattern {
                    name: name.to_owned(),
                });
            }
            None
        }
        _ => None,
    }
}

/// Why an enumerated text cannot be supplied: the argument's own pattern or lengths refuse it, so
/// the schema would list a value the boundary rejects. An enumeration is text-only, which is what
/// the boundary carries, so the member is compared as itself.
pub(crate) fn enum_member_violates(
    name: &str,
    definition: &Value,
    pattern: &Option<regex::Regex>,
    text: &str,
) -> Option<SchemaError> {
    if !default_allowed(pattern, text) {
        return Some(SchemaError::EnumValueOutsidePattern {
            name: name.to_owned(),
            value: text.to_owned(),
        });
    }
    let length = text.chars().count() as i64;
    let outside = match (
        stated_int(definition, "minLength"),
        stated_int(definition, "maxLength"),
    ) {
        (Some(low), _) if length < low => true,
        (_, Some(high)) if length > high => true,
        _ => false,
    };
    if outside {
        return Some(SchemaError::EnumValueOutsideLengths {
            name: name.to_owned(),
            value: text.to_owned(),
        });
    }
    None
}

/// The text a boolean default is carried as at the boundary.
fn bool_text(flag: bool) -> &'static str {
    if flag { "true" } else { "false" }
}

/// Whether the argument's compiled pattern accepts the default's transport text; with no
/// pattern there is nothing to satisfy.
pub(crate) fn default_allowed(pattern: &Option<regex::Regex>, text: &str) -> bool {
    match pattern {
        None => true,
        Some(compiled) => compiled.is_match(text),
    }
}

/// The compiled-regex size cap, matching the skill validator's, so a contract cannot ask the
/// hub to build an expensive matcher.
const MAX_PATTERN_BYTES: usize = 1 << 20;

/// Compiles one pattern the schema states, exactly as written and unanchored, which is how the
/// read-tool boundary matches it; a skill boundary anchors the same pattern with its own rule.
fn compile_pattern(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .size_limit(MAX_PATTERN_BYTES)
        .build()
        .map_err(|reason| format!("pattern does not compile: {reason}"))
}
