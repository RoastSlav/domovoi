//! Per-check settings from `[checks.<key>]` and per-check persistent state.

use std::collections::BTreeMap;

use domovoi_core::Criticality;
use jiff::SignedDuration;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::CheckError;

/// One check's settings as the agent read them from `[checks.<key>]`.
///
/// Thresholds are untyped here because this crate cannot see the config crate; each
/// check reads them into its own typed struct with [`CheckSettings::thresholds`].
#[derive(Debug, Clone, PartialEq)]
pub struct CheckSettings {
    /// Whether the check runs at all.
    pub enabled: bool,
    /// Overrides the check's default cadence.
    pub cadence: Option<SignedDuration>,
    /// Check-specific thresholds.
    pub thresholds: Map<String, Value>,
    /// Overrides how bad a failure is, for checks that honour it.
    pub criticality: Option<Criticality>,
}

impl Default for CheckSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            cadence: None,
            thresholds: Map::new(),
            criticality: None,
        }
    }
}

impl CheckSettings {
    /// Settings with the given thresholds and everything else at its default.
    #[must_use]
    pub fn with_thresholds(thresholds: Value) -> Self {
        Self {
            thresholds: match thresholds {
                Value::Object(map) => map,
                _ => Map::new(),
            },
            ..Self::default()
        }
    }

    /// The thresholds as `T`. Missing keys take `T`'s defaults; unknown or mistyped keys
    /// are a [`CheckError::Settings`] naming the key.
    pub fn thresholds<T: DeserializeOwned>(&self) -> Result<T, CheckError> {
        serde_json::from_value(Value::Object(self.thresholds.clone()))
            .map_err(|err| CheckError::Settings(err.to_string()))
    }
}

/// A small string map a check keeps between runs. The agent persists it.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct CheckState(BTreeMap<String, String>);

impl CheckState {
    /// An empty state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The value stored under `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// Stores `value` under `key`.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.0.insert(key.to_owned(), value.into());
    }

    /// Removes `key`.
    pub fn remove(&mut self, key: &str) {
        self.0.remove(key);
    }
}
