//! Values that may be one of the variants this build knows, or one it does not.

use std::fmt;

use serde::de::DeserializeOwned;
use serde_json::Value;

/// A protocol value that may be a variant this build knows, or one it does not.
///
/// It serialises as the raw string it decoded from, and remembers the parsed variant when
/// this build knows it. Use it only for data enums whose values can come from a skill, a
/// package or a later milestone; authority and state enums stay closed.
#[derive(Clone, PartialEq, Eq)]
pub struct Open<T> {
    raw: String,
    known: Option<T>,
}

impl<T> Open<T> {
    /// The variant this build knows, when it knows one.
    #[must_use]
    pub fn known(&self) -> Option<&T> {
        self.known.as_ref()
    }

    /// The value as it arrived, whether or not this build knows it.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Wraps a value this build produces.
    #[must_use]
    pub fn new(raw: impl Into<String>, known: Option<T>) -> Self {
        Self {
            raw: raw.into(),
            known,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Open<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Open")
            .field("raw", &self.raw)
            .field("known", &self.known)
            .finish()
    }
}

impl<T> serde::Serialize for Open<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

impl<'de, T: DeserializeOwned> serde::Deserialize<'de> for Open<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = <String as serde::Deserialize>::deserialize(deserializer)?;
        let known = serde_json::from_value(Value::String(raw.clone())).ok();
        Ok(Self { raw, known })
    }
}
