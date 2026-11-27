//! Validation and canonical text of fact values.

use serde_json::{Map, Value};

use crate::addr::canonical_cidr;

/// Why a value was refused for a predicate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValueError {
    /// The value has the wrong JSON type.
    #[error("the value has the wrong type")]
    WrongType,
    /// A string or number is outside its allowed range or length.
    #[error("the value is out of range")]
    OutOfRange,
    /// An object has a key the predicate does not define.
    #[error("the value has an unknown key")]
    UnknownKey,
    /// An object lacks a required key.
    #[error("the value lacks a required key")]
    MissingKey,
    /// A CIDR does not parse.
    #[error("the value is not a valid address range")]
    BadCidr,
    /// A range with no prefix (`0.0.0.0/0`, `::/0`) names every address, so it is no path
    /// and no trust boundary at all.
    #[error("a range that covers every address cannot be used here: name the network")]
    CoversEverything,
    /// A timestamp is not RFC 3339 UTC.
    #[error("the value is not a valid timestamp")]
    BadTimestamp,
    /// The predicate cannot describe this kind of subject.
    #[error("the predicate does not apply to this subject")]
    SubjectNotAllowed,
}

/// Compact JSON with object keys sorted bytewise, recursively. The stored text form of a
/// value: equal text means equal value whatever the `serde_json` feature set.
#[must_use]
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn sorted_keys(map: &Map<String, Value>) -> Vec<&String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    keys
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            out.push('{');
            for (i, key) in sorted_keys(map).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(inner) = map.get(key) {
                    write_canonical(inner, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Every string, number and boolean leaf of a value joined by a space, keys in sorted order.
/// This is what full-text search indexes.
#[must_use]
pub fn search_text(value: &Value) -> String {
    let mut leaves = Vec::new();
    collect_leaves(value, &mut leaves);
    leaves.join(" ")
}

fn collect_leaves(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Number(n) => out.push(n.to_string()),
        Value::Bool(b) => out.push(b.to_string()),
        Value::Array(items) => items.iter().for_each(|i| collect_leaves(i, out)),
        Value::Object(map) => {
            for key in sorted_keys(map) {
                if let Some(inner) = map.get(key) {
                    collect_leaves(inner, out);
                }
            }
        }
        Value::Null => {}
    }
}

/// A trimmed string of `min..=max` characters.
pub(super) fn text(value: &Value, min: usize, max: usize) -> Result<String, ValueError> {
    let s = value.as_str().ok_or(ValueError::WrongType)?.trim();
    let n = s.chars().count();
    if n < min || n > max {
        return Err(ValueError::OutOfRange);
    }
    Ok(s.to_owned())
}

/// The object of a value, checked against its required and optional keys.
pub(super) fn object<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a Map<String, Value>, ValueError> {
    let map = value.as_object().ok_or(ValueError::WrongType)?;
    if map
        .keys()
        .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(ValueError::UnknownKey);
    }
    if required.iter().any(|k| !map.contains_key(*k)) {
        return Err(ValueError::MissingKey);
    }
    Ok(map)
}

/// `{"cidr": ...}` with the range in canonical form.
pub(super) fn cidr_object(value: &Value) -> Result<Value, ValueError> {
    let map = object(value, &["cidr"], &[])?;
    let raw = map["cidr"].as_str().ok_or(ValueError::WrongType)?;
    let canonical = canonical_cidr(raw).ok_or(ValueError::BadCidr)?;
    if canonical.ends_with("/0") {
        return Err(ValueError::CoversEverything);
    }
    Ok(serde_json::json!({ "cidr": canonical }))
}

/// A UTC RFC 3339 timestamp.
pub(super) fn utc_timestamp(value: &Value) -> Result<jiff::Timestamp, ValueError> {
    let s = value.as_str().ok_or(ValueError::WrongType)?;
    s.parse().map_err(|_| ValueError::BadTimestamp)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let a = json!({"b": 1, "a": {"y": [1, {"d": 1, "c": 2}], "x": "s"}});
        let b = json!({"a": {"x": "s", "y": [1, {"c": 2, "d": 1}]}, "b": 1});
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(
            canonical_json(&a),
            r#"{"a":{"x":"s","y":[1,{"c":2,"d":1}]},"b":1}"#
        );
    }

    #[test]
    fn search_text_lists_leaves_in_key_order() {
        assert_eq!(
            search_text(&json!({"b": "two", "a": 1, "c": true})),
            "1 two true"
        );
    }
}
