//! Evidence attached to a signal: measurements and context, never part of a fingerprint.

use std::collections::BTreeMap;

/// One evidence value.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum EvidenceValue {
    /// Free text.
    Text(String),
    /// A whole number.
    Int(i64),
    /// A fractional number.
    Float(f64),
    /// A flag.
    Bool(bool),
}

/// An ordered key to value map describing what a check saw.
///
/// The reserved key [`Evidence::VALUE_KEY`] holds the tracked numeric value used by delta checks.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Evidence(BTreeMap<String, EvidenceValue>);

impl Evidence {
    /// Reserved key for the tracked numeric value.
    pub const VALUE_KEY: &'static str = "value";

    /// Creates an empty evidence map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `key`, replacing any previous value.
    pub fn insert(&mut self, key: impl Into<String>, value: EvidenceValue) {
        self.0.insert(key.into(), value);
    }

    /// Looks up `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&EvidenceValue> {
        self.0.get(key)
    }

    /// Iterates entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &EvidenceValue)> {
        self.0.iter()
    }

    /// The tracked numeric value, if the reserved `value` key holds a number.
    #[must_use]
    pub fn tracked_value(&self) -> Option<f64> {
        match self.get(Self::VALUE_KEY)? {
            EvidenceValue::Int(i) => Some(*i as f64),
            EvidenceValue::Float(f) => Some(*f),
            EvidenceValue::Text(_) | EvidenceValue::Bool(_) => None,
        }
    }
}
