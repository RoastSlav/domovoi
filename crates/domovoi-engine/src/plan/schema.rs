//! A small JSON Schema check for capability arguments.
//!
//! Supports exactly the keywords the built-in and skill schemas use. Arguments are canonical
//! strings, so `integer` means the text parses as `i64` and `boolean` means `true` or `false`.

use std::collections::BTreeMap;

use regex::RegexBuilder;
use serde_json::Value;

/// Keywords this check understands. Anything else is ignored, and a test fails when a schema
/// in use falls outside this set.
#[cfg(test)]
pub(super) const SUPPORTED_KEYWORDS: [&str; 13] = [
    "type",
    "properties",
    "required",
    "additionalProperties",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "enum",
    "description",
    "default",
    "title",
];

/// One thing wrong with the arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaProblem {
    /// The argument concerned.
    pub path: String,
    /// What is wrong, in one short sentence.
    pub problem: String,
}

/// Checks `args` against `schema`.
pub fn check_schema(
    schema: &Value,
    args: &BTreeMap<String, String>,
) -> Result<(), Vec<SchemaProblem>> {
    let mut problems = Vec::new();
    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !args.contains_key(key) {
                problems.push(problem(key, format!("missing required `{key}`")));
            }
        }
    }
    let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
    for (key, value) in args {
        match properties.and_then(|p| p.get(key)) {
            Some(spec) => check_value(key, spec, value, &mut problems),
            None if closed => problems.push(problem(key, format!("unexpected `{key}`"))),
            None => {}
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn problem(path: &str, text: String) -> SchemaProblem {
    SchemaProblem {
        path: path.to_owned(),
        problem: text,
    }
}

fn check_value(key: &str, spec: &Value, value: &str, out: &mut Vec<SchemaProblem>) {
    let kind = spec.get("type").and_then(Value::as_str);
    let length = value.chars().count() as u64;
    match kind {
        Some("integer") => match value.parse::<i64>() {
            Ok(number) => {
                if let Some(min) = spec.get("minimum").and_then(Value::as_i64)
                    && number < min
                {
                    out.push(problem(key, format!("`{key}` must be at least {min}")));
                }
                if let Some(max) = spec.get("maximum").and_then(Value::as_i64)
                    && number > max
                {
                    out.push(problem(key, format!("`{key}` must be at most {max}")));
                }
            }
            Err(_) => out.push(problem(key, format!("`{key}` must be an integer"))),
        },
        Some("boolean") => {
            if value != "true" && value != "false" {
                out.push(problem(key, format!("`{key}` must be true or false")));
            }
        }
        _ => {}
    }
    if let Some(min) = spec.get("minLength").and_then(Value::as_u64)
        && length < min
    {
        out.push(problem(
            key,
            format!("`{key}` must be at least {min} characters"),
        ));
    }
    if let Some(max) = spec.get("maxLength").and_then(Value::as_u64)
        && length > max
    {
        out.push(problem(
            key,
            format!("`{key}` must be at most {max} characters"),
        ));
    }
    if let Some(pattern) = spec.get("pattern").and_then(Value::as_str) {
        match RegexBuilder::new(pattern).size_limit(1 << 20).build() {
            Ok(regex) => {
                if !regex.is_match(value) {
                    out.push(problem(key, format!("`{key}` does not match its pattern")));
                }
            }
            Err(_) => out.push(problem(
                key,
                "the capability's schema has an unusable pattern".to_owned(),
            )),
        }
    }
    if let Some(options) = spec.get("enum").and_then(Value::as_array) {
        let names: Vec<&str> = options.iter().filter_map(Value::as_str).collect();
        if !names.contains(&value) {
            out.push(problem(
                key,
                format!("`{key}` must be one of {}", names.join(", ")),
            ));
        }
    }
}
