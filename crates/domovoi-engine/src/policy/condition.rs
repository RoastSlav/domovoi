//! Conditions on a request. They decide only whether a rule matches.

use domovoi_core::capability::{ConstraintKind, FactId};
use jiff::SignedDuration;
use jiff::tz::TimeZone;
use serde::{Deserialize, Deserializer, Serialize};

use super::request::EvaluateRequest;

/// A time of day, no date, read in UTC.
///
/// UTC keeps a decision identical across hosts and replays and avoids daylight-saving
/// ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimeOfDay {
    /// Hour, `0..=23`.
    pub hour: u8,
    /// Minute, `0..=59`.
    pub minute: u8,
}

impl TimeOfDay {
    fn minutes(self) -> u16 {
        u16::from(self.hour) * 60 + u16::from(self.minute)
    }

    /// The time as `HH:MM`.
    #[must_use]
    pub fn label(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// A condition on a request. Conditions never affect specificity; they decide only whether a
/// rule matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    /// The canonical argument `arg` is an integer satisfying `value <op> n`.
    ArgConstraint {
        /// The argument name.
        arg: String,
        /// Whether `value` is a minimum or a maximum.
        #[serde(deserialize_with = "deserialize_kind")]
        op: ConstraintKind,
        /// The bound.
        value: i64,
    },
    /// The UTC wall clock of the request's `now`, inside `[start, end)`.
    TimeWindow {
        /// Inclusive start.
        start: TimeOfDay,
        /// Exclusive end; an earlier end wraps across midnight.
        end: TimeOfDay,
    },
    /// The fact is `Known` and equal to `expected`.
    RequiresFact {
        /// The fact.
        fact: FactId,
        /// The value it must be known to have.
        expected: bool,
    },
    /// The requested rate is at most `max_per_hour`.
    MaxRate {
        /// The bound.
        max_per_hour: u32,
    },
    /// The requested TTL is at most `max_ttl`.
    MaxTtl {
        /// The bound.
        max_ttl: SignedDuration,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum KindMirror {
    IntAtLeast,
    IntAtMost,
}

fn deserialize_kind<'de, D: Deserializer<'de>>(d: D) -> Result<ConstraintKind, D::Error> {
    Ok(match KindMirror::deserialize(d)? {
        KindMirror::IntAtLeast => ConstraintKind::IntAtLeast,
        KindMirror::IntAtMost => ConstraintKind::IntAtMost,
    })
}

/// Whether `n` satisfies the bound under `kind`.
#[must_use]
pub(crate) fn satisfied(kind: ConstraintKind, n: i64, bound: i64) -> bool {
    match kind {
        ConstraintKind::IntAtLeast => n >= bound,
        ConstraintKind::IntAtMost => n <= bound,
    }
}

impl Condition {
    /// Whether this condition holds for `request`.
    ///
    /// A missing, malformed or out-of-range input never holds, so a rule falls through to
    /// broader rules or the default and never to a more permissive one.
    #[must_use]
    pub fn holds(&self, request: &EvaluateRequest) -> bool {
        match self {
            Self::ArgConstraint { arg, op, value } => request
                .args
                .get(arg)
                .and_then(|text| text.parse::<i64>().ok())
                .is_some_and(|n| satisfied(*op, n, *value)),
            Self::TimeWindow { start, end } => {
                let zoned = request.now.to_zoned(TimeZone::UTC);
                let t = u16::try_from(zoned.hour()).unwrap_or(0) * 60
                    + u16::try_from(zoned.minute()).unwrap_or(0);
                let (s, e) = (start.minutes(), end.minutes());
                if s < e {
                    s <= t && t < e
                } else if s > e {
                    t >= s || t < e
                } else {
                    false
                }
            }
            Self::RequiresFact { fact, expected } => {
                request.facts.get(fact) == Some(&FactState::Known(*expected))
            }
            Self::MaxRate { max_per_hour } => request
                .requested_rate_per_hour
                .is_none_or(|rate| rate <= *max_per_hour),
            Self::MaxTtl { max_ttl } => request.requested_ttl.is_none_or(|ttl| ttl <= *max_ttl),
        }
    }
}

/// What the hub knows about one safety-critical fact, as supplied by the caller.
///
/// A policy fact is a boolean predicate about the hub's world, for example
/// `host.not_in_maintenance`. It is deliberately not the digest's display value and not
/// incident evidence. Only `Known(expected)` ever satisfies a requirement: an absent fact
/// must never read as the safe value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FactState {
    /// Known, with its value.
    Known(bool),
    /// Never observed, or unreadable.
    Unknown,
    /// Last observation is older than the fact's freshness bound.
    Stale,
    /// Two sources disagree.
    Disputed,
}

impl FactState {
    /// The same state with the value dropped, for the trace.
    #[must_use]
    pub const fn status(self) -> FactStatus {
        match self {
            Self::Known(_) => FactStatus::Known,
            Self::Unknown => FactStatus::Unknown,
            Self::Stale => FactStatus::Stale,
            Self::Disputed => FactStatus::Disputed,
        }
    }
}

/// The four fact states with the value dropped. The trace records this, never the observed
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    /// Known.
    Known,
    /// Never observed.
    Unknown,
    /// Too old.
    Stale,
    /// Sources disagree.
    Disputed,
}
