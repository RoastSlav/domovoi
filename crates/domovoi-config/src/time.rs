//! Wall-clock times of day and IANA time zones.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use jiff::civil::Time;
use schemars::{JsonSchema, Schema, SchemaGenerator};

/// A time of day written as `"HH:MM"` in 24-hour form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalTime(Time);

impl LocalTime {
    /// The time of day.
    #[must_use]
    pub fn time(self) -> Time {
        self.0
    }

    /// A default time; callers pass a valid constant.
    pub(crate) const fn hm(hour: i8, minute: i8) -> Self {
        Self(jiff::civil::time(hour, minute, 0, 0))
    }
}

/// Why a time of day or time zone was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TimeError(String);

impl FromStr for LocalTime {
    type Err = TimeError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bad = || {
            TimeError(format!(
                "`{input}` must be a 24-hour time such as \"09:00\""
            ))
        };
        let (h, m) = input.split_once(':').ok_or_else(bad)?;
        if h.len() != 2 || m.len() != 2 {
            return Err(bad());
        }
        let hour: i8 = h.parse().map_err(|_| bad())?;
        let minute: i8 = m.parse().map_err(|_| bad())?;
        Time::new(hour, minute, 0, 0).map(Self).map_err(|_| bad())
    }
}

impl fmt::Display for LocalTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.0.hour(), self.0.minute())
    }
}

impl<'de> serde::Deserialize<'de> for LocalTime {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for LocalTime {
    fn schema_name() -> Cow<'static, str> {
        "LocalTime".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = String::json_schema(generator);
        schema.insert("pattern".to_owned(), "^[0-2][0-9]:[0-5][0-9]$".into());
        schema.insert(
            "description".to_owned(),
            "A 24-hour time of day such as \"09:00\".".into(),
        );
        schema
    }
}

/// An IANA time zone name such as `Europe/Sofia` or `UTC`, checked against the time zone
/// database when loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeZone {
    name: String,
    zone: jiff::tz::TimeZone,
}

impl TimeZone {
    /// The name as written in the file.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The resolved zone.
    #[must_use]
    pub fn zone(&self) -> &jiff::tz::TimeZone {
        &self.zone
    }

    /// Coordinated Universal Time.
    #[must_use]
    pub fn utc() -> Self {
        Self {
            name: "UTC".to_owned(),
            zone: jiff::tz::TimeZone::UTC,
        }
    }
}

impl FromStr for TimeZone {
    type Err = TimeError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        jiff::tz::TimeZone::get(input)
            .map(|zone| Self {
                name: input.to_owned(),
                zone,
            })
            .map_err(|_| {
                TimeError(format!(
                    "`{input}` is not a known IANA time zone; use a name such as \"Europe/Sofia\" or \"UTC\""
                ))
            })
    }
}

impl<'de> serde::Deserialize<'de> for TimeZone {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for TimeZone {
    fn schema_name() -> Cow<'static, str> {
        "TimeZone".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = String::json_schema(generator);
        schema.insert(
            "description".to_owned(),
            "An IANA time zone name such as \"Europe/Sofia\" or \"UTC\".".into(),
        );
        schema
    }
}
