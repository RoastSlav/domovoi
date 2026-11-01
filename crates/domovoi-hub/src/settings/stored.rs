//! The UI settings layer as kept in the `settings` table.
//!
//! - `config`: `{ revision, document }`, the document shaped like `hub.toml`, with
//!   `{ "stored": true }` where a secret was entered in the UI.
//! - `config.previous`: the document before the last save, for rollback.
//! - `secret.<path>`: each UI secret, sealed with the data directory's key, as base64.
//! - `host.<name>.checks`: per-host check overrides.
//! - `last_apply`: how the last apply went.
//!
//! Only the settings service writes these keys.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use domovoi_config::StoredSecrets;
use domovoi_core::ports::{SettingsRepo, StoreError};
use secrecy::SecretString;
use serde_json::{Map, Value, json};

use super::crypto::SecretKey;

/// The current UI document.
pub const CONFIG: &str = "config";
/// The document before the last save.
pub const PREVIOUS: &str = "config.previous";
/// Prefix of sealed secrets.
pub const SECRET_PREFIX: &str = "secret.";
/// The outcome of the last apply.
pub const LAST_APPLY: &str = "last_apply";
/// UI-only first-run state; never part of the configuration.
pub const FIRST_RUN: &str = "ui.first_run";

/// The key of a host's check overrides.
#[must_use]
pub fn host_checks_key(host: &str) -> String {
    format!("host.{host}.checks")
}

/// A stored document and its revision. Revision 0 is "nothing saved yet".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoredDoc {
    /// Bumped on every save.
    pub revision: u64,
    /// The values saved from the UI, shaped like `hub.toml`.
    pub document: Map<String, Value>,
}

impl StoredDoc {
    pub(crate) fn from_value(value: Option<Value>) -> Self {
        let Some(Value::Object(mut object)) = value else {
            return Self::default();
        };
        let revision = object.get("revision").and_then(Value::as_u64).unwrap_or(0);
        let document = match object.remove("document") {
            Some(Value::Object(document)) => document,
            _ => Map::new(),
        };
        Self { revision, document }
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({ "revision": self.revision, "document": self.document })
    }

    /// The document as a TOML table for the layered loader.
    pub fn table(&self) -> Result<toml::Table, String> {
        let mut table = toml::Table::new();
        for (key, value) in &self.document {
            if let Some(value) = json_to_toml(value)? {
                table.insert(key.clone(), value);
            }
        }
        Ok(table)
    }
}

/// The stored layer with its secrets decrypted.
#[derive(Default)]
pub struct StoredState {
    /// The current document.
    pub doc: StoredDoc,
    /// Decrypted UI secrets by config path.
    pub secrets: BTreeMap<String, SecretString>,
    /// Paths whose ciphertext could not be opened (lost or replaced key).
    pub unreadable: Vec<String>,
}

impl std::fmt::Debug for StoredState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredState")
            .field("doc", &self.doc)
            .field("secrets", &self.secrets.keys().collect::<Vec<_>>())
            .field("unreadable", &self.unreadable)
            .finish()
    }
}

impl StoredSecrets for StoredState {
    fn get(&self, path: &str) -> Option<SecretString> {
        self.secrets.get(path).cloned()
    }
}

impl StoredState {
    /// Reads the document and opens every stored secret with `key`.
    pub async fn load(repo: &dyn SettingsRepo, key: &SecretKey) -> Result<Self, StoreError> {
        let doc = StoredDoc::from_value(repo.get(CONFIG).await?);
        let mut secrets = BTreeMap::new();
        let mut unreadable = Vec::new();
        for (name, value) in repo.list_prefix(SECRET_PREFIX).await? {
            let Some(path) = name.strip_prefix(SECRET_PREFIX) else {
                continue;
            };
            let opened = value
                .as_str()
                .and_then(|text| STANDARD.decode(text).ok())
                .and_then(|bytes| key.open(path, &bytes).ok());
            match opened {
                Some(secret) => {
                    secrets.insert(path.to_owned(), secret);
                }
                None => unreadable.push(path.to_owned()),
            }
        }
        Ok(Self {
            doc,
            secrets,
            unreadable,
        })
    }
}

/// A sealed secret as stored under `secret.<path>`.
pub(crate) fn sealed_value(
    key: &SecretKey,
    path: &str,
    secret: &SecretString,
) -> Result<Value, super::crypto::CryptoError> {
    Ok(Value::String(STANDARD.encode(key.seal(path, secret)?)))
}

/// Converts JSON to TOML. `null` has no TOML form, so it is dropped (`Ok(None)`).
pub(crate) fn json_to_toml(value: &Value) -> Result<Option<toml::Value>, String> {
    Ok(Some(match value {
        Value::Null => return Ok(None),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                return Err(format!("{n} is out of range"));
            }
        }
        Value::String(s) => toml::Value::String(s.clone()),
        Value::Array(items) => toml::Value::Array(
            items
                .iter()
                .filter_map(|item| json_to_toml(item).transpose())
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => {
            let mut table = toml::Table::new();
            for (key, item) in map {
                if let Some(item) = json_to_toml(item)? {
                    table.insert(key.clone(), item);
                }
            }
            toml::Value::Table(table)
        }
    }))
}

/// Converts TOML to JSON, for showing merged values in the UI.
pub(crate) fn toml_to_json(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => json!(f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(items) => Value::Array(items.iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{StoredDoc, json_to_toml, toml_to_json};

    #[test]
    fn documents_round_trip_and_nulls_drop() {
        let value = json!({ "a": 1, "b": null, "c": [1.5, "x"], "d": { "e": true } });
        let toml = json_to_toml(&value).unwrap().unwrap();
        assert!(toml.get("b").is_none());
        assert_eq!(
            toml_to_json(&toml),
            json!({ "a": 1, "c": [1.5, "x"], "d": { "e": true } })
        );
        let doc = StoredDoc::from_value(Some(
            json!({ "revision": 3, "document": { "hub": { "name": "x" } } }),
        ));
        assert_eq!(doc.revision, 3);
        assert_eq!(StoredDoc::from_value(Some(doc.to_value())), doc);
        assert_eq!(StoredDoc::from_value(None).revision, 0);
    }
}
