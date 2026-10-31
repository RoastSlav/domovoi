//! Positive durations written as friendly strings.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use jiff::{SignedDuration, Span, SpanRelativeTo};
use schemars::{JsonSchema, Schema, SchemaGenerator};

/// A positive length of time written as `"30s"`, `"5m"`, `"24h"`, `"7d"` or `"1h30m"`.
///
/// Days are 24 hours. Zero and negative values are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(SignedDuration);

/// Why a duration string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DurationError(String);

const HINT: &str = "must be a positive duration such as \"30s\", \"5m\", \"24h\" or \"1h30m\"";

impl Duration {
    /// The duration as a `jiff::SignedDuration`; always positive.
    #[must_use]
    pub fn as_signed(self) -> SignedDuration {
        self.0
    }

    /// A duration of `secs` seconds, or `None` unless `secs` is positive.
    #[must_use]
    pub fn from_secs(secs: i64) -> Option<Self> {
        (secs > 0).then(|| Self(SignedDuration::from_secs(secs)))
    }

    /// A default duration; callers pass a positive constant.
    pub(crate) const fn secs(secs: i64) -> Self {
        Self(SignedDuration::from_secs(secs))
    }
}

impl FromStr for Duration {
    type Err = DurationError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let span: Span = input
            .trim()
            .parse()
            .map_err(|_| DurationError(format!("`{input}` {HINT}")))?;
        let duration = span
            .to_duration(SpanRelativeTo::days_are_24_hours())
            .map_err(|_| DurationError(format!("`{input}` {HINT}; weeks are the largest unit")))?;
        if duration.is_positive() {
            Ok(Self(duration))
        } else {
            Err(DurationError(format!("`{input}` {HINT}")))
        }
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Duration {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Duration {
    fn schema_name() -> Cow<'static, str> {
        "Duration".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = String::json_schema(generator);
        schema.insert(
            "description".to_owned(),
            "A positive duration such as \"30s\", \"5m\", \"24h\", \"7d\" or \"1h30m\".".into(),
        );
        schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_round_trips() {
        let d: Duration = "1h30m".parse().unwrap();
        assert_eq!(d.to_string().parse::<Duration>().unwrap(), d);
    }
}
