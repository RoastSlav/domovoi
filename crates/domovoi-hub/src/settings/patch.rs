//! Applying a settings patch to the stored document.
//!
//! A patch is a JSON object shaped like `hub.toml`. Objects merge key by key, `null`
//! removes a key (the lower layers then apply), and any other value replaces. A secret
//! field takes exactly one of `{ "keep": true }`, `{ "set": "<value>" }` or
//! `{ "clear": true }`; the value never enters the document, which records
//! `{ "stored": true }` instead.

use std::collections::BTreeMap;

use secrecy::SecretString;
use serde_json::{Map, Value, json};

/// What happens to a secret.
#[derive(Clone)]
pub enum SecretOp {
    /// Store this new value.
    Set(SecretString),
    /// Remove the stored value.
    Clear,
}

impl std::fmt::Debug for SecretOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Set(_) => "Set(..)",
            Self::Clear => "Clear",
        })
    }
}

/// One changed field.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A plain value, before and after (`None` when absent).
    Plain {
        /// Dotted path.
        path: String,
        /// The value before.
        old: Option<Value>,
        /// The value after.
        new: Option<Value>,
    },
    /// A secret was set or cleared; its value is never recorded.
    Secret {
        /// Dotted path.
        path: String,
        /// True when set, false when cleared.
        set: bool,
    },
}

impl Change {
    /// The field's dotted path.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Plain { path, .. } | Self::Secret { path, .. } => path,
        }
    }
}

/// The document after a patch, what changed, and the secret operations to store.
#[derive(Debug)]
pub struct Patched {
    /// The new document.
    pub document: Map<String, Value>,
    /// Every changed field.
    pub changes: Vec<Change>,
    /// Secrets to seal or remove, by path.
    pub secrets: BTreeMap<String, SecretOp>,
}

/// Why a patch is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchError {
    /// Where.
    pub path: String,
    /// What is wrong.
    pub message: String,
}

/// Applies `patch` to a copy of `document`.
pub fn apply(
    document: &Map<String, Value>,
    patch: &Map<String, Value>,
) -> Result<Patched, PatchError> {
    let mut next = document.clone();
    let mut secrets = BTreeMap::new();
    merge(&mut next, patch, "", &mut secrets)?;
    let mut changes = diff(document, &next);
    changes.retain(|c| !secrets.contains_key(c.path()));
    for (path, op) in &secrets {
        changes.push(Change::Secret {
            path: path.clone(),
            set: matches!(op, SecretOp::Set(_)),
        });
    }
    changes.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(Patched {
        document: next,
        changes,
        secrets,
    })
}

/// Whether changing `path` widens who can act or what is trusted, so the admin must
/// enter their password again.
#[must_use]
pub fn is_sensitive(change: &Change) -> bool {
    let path = change.path();
    matches!(change, Change::Secret { .. })
        || path == "discord.allowed_user_ids"
        || path == "web.trusted_proxies"
        || path == "webhooks"
        || path.starts_with("webhooks.")
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_owned()
    } else {
        format!("{prefix}.{key}")
    }
}

/// `Some` when `value` is exactly a secret operation: one key, `keep` or `clear` set to
/// `true`, or `set` with a string. Anything else is an ordinary table, so a setting such
/// as `backup.keep = 7` is never mistaken for one.
fn secret_op(
    value: &Map<String, Value>,
    path: &str,
) -> Result<Option<Option<SecretOp>>, PatchError> {
    if value.len() != 1 {
        return Ok(None);
    }
    Ok(match value.iter().next() {
        Some((key, Value::Bool(true))) if key == "keep" => Some(None),
        Some((key, Value::Bool(true))) if key == "clear" => Some(Some(SecretOp::Clear)),
        Some((key, Value::String(s))) if key == "set" => {
            if s.is_empty() {
                return Err(PatchError {
                    path: path.to_owned(),
                    message: "a secret cannot be empty; use { clear: true } to remove it"
                        .to_owned(),
                });
            }
            Some(Some(SecretOp::Set(SecretString::from(s.clone()))))
        }
        _ => None,
    })
}

fn merge(
    doc: &mut Map<String, Value>,
    patch: &Map<String, Value>,
    prefix: &str,
    secrets: &mut BTreeMap<String, SecretOp>,
) -> Result<(), PatchError> {
    for (key, value) in patch {
        let path = join(prefix, key);
        match value {
            Value::Null => {
                doc.remove(key);
            }
            Value::Object(inner) => match secret_op(inner, &path)? {
                Some(None) => {}
                Some(Some(SecretOp::Clear)) => {
                    doc.remove(key);
                    secrets.insert(path, SecretOp::Clear);
                }
                Some(Some(op)) => {
                    doc.insert(key.clone(), json!({ "stored": true }));
                    secrets.insert(path, op);
                }
                None => {
                    let slot = doc
                        .entry(key.clone())
                        .or_insert_with(|| Value::Object(Map::new()));
                    if !slot.is_object() || is_stored_marker(slot) {
                        *slot = Value::Object(Map::new());
                    }
                    if let Value::Object(child) = slot {
                        merge(child, inner, &path, secrets)?;
                        if child.is_empty() {
                            doc.remove(key);
                        }
                    }
                }
            },
            other => {
                doc.insert(key.clone(), other.clone());
            }
        }
    }
    Ok(())
}

fn is_stored_marker(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.len() == 1 && o.get("stored") == Some(&Value::Bool(true)))
}

fn leaves(map: &Map<String, Value>, prefix: &str, out: &mut BTreeMap<String, Value>) {
    for (key, value) in map {
        let path = join(prefix, key);
        match value {
            Value::Object(inner) if !is_stored_marker(value) => leaves(inner, &path, out),
            other => {
                out.insert(path, other.clone());
            }
        }
    }
}

fn diff(old: &Map<String, Value>, new: &Map<String, Value>) -> Vec<Change> {
    let (mut before, mut after) = (BTreeMap::new(), BTreeMap::new());
    leaves(old, "", &mut before);
    leaves(new, "", &mut after);
    let mut paths: Vec<&String> = before.keys().chain(after.keys()).collect();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .filter(|p| before.get(*p) != after.get(*p))
        .map(|p| Change::Plain {
            path: p.clone(),
            old: before.get(p).cloned(),
            new: after.get(p).cloned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;
    use serde_json::{Map, Value, json};

    use super::{Change, SecretOp, apply, is_sensitive};

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    #[test]
    fn patches_merge_remove_and_record_secrets_without_values() {
        let doc = map(
            json!({ "hub": { "name": "a", "timezone": "UTC" }, "llm": { "primary": { "api_key": { "stored": true } } } }),
        );
        let patch = map(json!({
            "hub": { "name": "b", "timezone": null },
            "discord": { "bot_token": { "set": "tok-value" }, "channel_id": 5 },
            "llm": { "primary": { "api_key": { "keep": true } } }
        }));
        let out = apply(&doc, &patch).unwrap();
        assert_eq!(out.document["hub"], json!({ "name": "b" }));
        assert_eq!(
            out.document["discord"]["bot_token"],
            json!({ "stored": true })
        );
        assert!(matches!(
            out.secrets.get("discord.bot_token"),
            Some(SecretOp::Set(s)) if s.expose_secret() == "tok-value"
        ));
        assert!(
            !out.secrets.contains_key("llm.primary.api_key"),
            "keep changes nothing"
        );
        let paths: Vec<_> = out.changes.iter().map(Change::path).collect();
        assert_eq!(
            paths,
            [
                "discord.bot_token",
                "discord.channel_id",
                "hub.name",
                "hub.timezone"
            ]
        );
        assert!(!format!("{:?}", out.changes).contains("tok-value"));
        assert!(is_sensitive(&out.changes[0]));
        assert!(!is_sensitive(&out.changes[1]));

        let cleared = apply(
            &doc,
            &map(json!({ "llm": { "primary": { "api_key": { "clear": true } } } })),
        )
        .unwrap();
        assert!(
            cleared.document.get("llm").is_none(),
            "an emptied table goes away"
        );
        assert!(matches!(
            cleared.secrets.get("llm.primary.api_key"),
            Some(SecretOp::Clear)
        ));
        assert!(apply(&doc, &map(json!({ "heartbeat": { "url": { "set": "" } } }))).is_err());
        let backup = apply(&doc, &map(json!({ "backup": { "keep": 3 } }))).unwrap();
        assert_eq!(
            backup.document["backup"],
            json!({ "keep": 3 }),
            "not a secret operation"
        );
        assert!(backup.secrets.is_empty());
    }
}
