//! The full examples show every key of the configuration schema, so a new setting cannot be
//! added without appearing in the file people copy from.

use std::collections::BTreeSet;
use std::path::PathBuf;

use domovoi_config::{AgentConfig, HubConfig};
use serde_json::{Map, Value};

fn example_text(name: &str) -> std::io::Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    std::fs::read_to_string(path)
}

/// Follows `$ref` and `Option` (`anyOf` with null) to the schema that describes the value.
fn resolve<'a>(defs: &'a Map<String, Value>, node: &'a Value) -> &'a Value {
    if let Some(name) = node
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
        && let Some(def) = defs.get(name)
    {
        return resolve(defs, def);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(options) = node.get(key).and_then(Value::as_array) {
            let mut real = options
                .iter()
                .filter(|o| o.get("type").and_then(Value::as_str) != Some("null"));
            if let (Some(only), None) = (real.next(), real.next()) {
                return resolve(defs, only);
            }
        }
    }
    node
}

/// The table behind a value: the value itself, the items of a list, or the entries of a map.
fn table<'a>(defs: &'a Map<String, Value>, node: &'a Value) -> Option<&'a Value> {
    let node = resolve(defs, node);
    if node.get("properties").is_some() {
        return Some(node);
    }
    if let Some(items) = node.get("items") {
        return table(defs, items);
    }
    node.get("additionalProperties")
        .filter(|v| v.is_object())
        .and_then(|v| table(defs, v))
}

/// Every key path the schema allows, with `*` for the name of a map entry.
fn schema_paths(defs: &Map<String, Value>, node: &Value, prefix: &str, out: &mut BTreeSet<String>) {
    let Some(node) = table(defs, node) else {
        return;
    };
    let Some(props) = node.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (key, child) in props {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        out.insert(path.clone());
        let is_map_of_tables = {
            let r = resolve(defs, child);
            r.get("properties").is_none()
                && r.get("additionalProperties")
                    .filter(|v| v.is_object())
                    .is_some_and(|v| table(defs, v).is_some())
        };
        let next = if is_map_of_tables {
            format!("{path}.*")
        } else {
            path
        };
        schema_paths(defs, child, &next, out);
    }
}

/// The table that holds `segment` below `node`: a named key, or the entry of a map.
fn descend<'a>(
    defs: &'a Map<String, Value>,
    node: &'a Value,
    segment: &str,
) -> Option<(String, &'a Value)> {
    let resolved = resolve(defs, node);
    if let Some(child) = resolved.get("properties").and_then(|p| p.get(segment)) {
        return Some((segment.to_owned(), child));
    }
    resolved
        .get("additionalProperties")
        .filter(|v| v.is_object())
        .map(|v| ("*".to_owned(), v))
}

/// The schema a header's next segment is looked up in: a map stays a map, a list is its items.
fn step_into<'a>(defs: &'a Map<String, Value>, node: &'a Value) -> &'a Value {
    let resolved = resolve(defs, node);
    if resolved.get("properties").is_none()
        && let Some(items) = resolved.get("items")
    {
        return resolve(defs, items);
    }
    resolved
}

fn split_header(header: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in header.chars() {
        match c {
            '"' => quoted = !quoted,
            '.' if !quoted => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
}

/// The identifiers written as `name =` at the top level of an inline table.
fn inline_keys(value: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let (mut depth, mut quoted) = (0_usize, false);
    let mut word = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            quoted = c != '"';
            continue;
        }
        match c {
            '"' => quoted = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth = depth.saturating_sub(1),
            c if c.is_ascii_alphanumeric() || c == '_' => word.push(c),
            c => {
                if depth == 1 && c.is_whitespace() || c == '=' {
                    while chars.peek().is_some_and(|n| n.is_whitespace()) {
                        chars.next();
                    }
                    if (c == '=' || chars.peek() == Some(&'=')) && !word.is_empty() && depth == 1 {
                        keys.push(word.clone());
                    }
                }
                word.clear();
            }
        }
    }
    keys
}

/// Every key path the example writes, in code or in a comment that shows a setting.
fn text_paths(schema: &Value, text: &str) -> BTreeSet<String> {
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut found = BTreeSet::new();
    let mut section: Vec<String> = Vec::new();
    let mut node: Option<&Value> = Some(schema);
    for raw in text.lines() {
        let line = raw.trim();
        let line = line
            .strip_prefix('#')
            .map_or(line, |rest| rest.strip_prefix(' ').unwrap_or(rest))
            .trim();
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_start_matches('[');
            let Some(end) = header.find(']') else {
                continue;
            };
            let names = split_header(&header[..end]);
            section.clear();
            let mut at = Some(schema);
            for name in &names {
                match at.and_then(|n| descend(&defs, n, name)) {
                    Some((canonical, child)) => {
                        section.push(canonical);
                        found.insert(section.join("."));
                        at = Some(step_into(&defs, child));
                    }
                    None => {
                        at = None;
                        break;
                    }
                }
            }
            node = at;
            continue;
        }
        let name: String = line
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let rest = line[name.len()..].trim_start();
        if name.is_empty() || !rest.starts_with('=') || rest.starts_with("==") {
            continue;
        }
        let Some((canonical, child)) = node.and_then(|n| descend(&defs, n, &name)) else {
            continue;
        };
        let path = section
            .iter()
            .cloned()
            .chain([canonical])
            .collect::<Vec<_>>()
            .join(".");
        found.insert(path.clone());
        if let Some(inner) = table(&defs, child) {
            for key in inline_keys(rest[1..].trim()) {
                if let Some((c, _)) = descend(&defs, inner, &key) {
                    found.insert(format!("{path}.{c}"));
                }
            }
        }
    }
    found
}

fn missing(schema: &Value, example: &str) -> std::io::Result<Vec<String>> {
    let defs = schema
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut wanted = BTreeSet::new();
    schema_paths(&defs, schema, "", &mut wanted);
    let shown = text_paths(schema, &example_text(example)?);
    Ok(wanted.difference(&shown).cloned().collect())
}

#[test]
fn hub_full_example_shows_every_key() {
    let schema = serde_json::to_value(HubConfig::json_schema()).unwrap();
    let missing = missing(&schema, "hub.full.toml").unwrap();
    assert!(
        missing.is_empty(),
        "examples/hub.full.toml does not show: {missing:#?}"
    );
}

#[test]
fn agent_example_shows_every_key() {
    let schema = serde_json::to_value(AgentConfig::json_schema()).unwrap();
    let missing = missing(&schema, "agent.toml").unwrap();
    assert!(
        missing.is_empty(),
        "examples/agent.toml does not show: {missing:#?}"
    );
}
