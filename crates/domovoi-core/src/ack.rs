//! Ack rules and maintenance windows: ways to keep known conditions from notifying.

use std::fmt;

use jiff::Timestamp;

use crate::error::ValidationError;
use crate::id::{AckRuleId, HostId};
use crate::signal::{CheckKey, EvidenceValue, Signal};

/// A target pattern with an optional `*` wildcard at the start and/or end only.
///
/// `plex` matches exactly, `plex*` matches by prefix, `*-backup` by suffix, `*cache*` by
/// substring, and `*` matches any target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TargetPattern(String);

impl TargetPattern {
    /// Validates `input`: non-empty, with `*` only as the first and/or last character.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.is_empty() {
            return Err(ValidationError::new("target pattern", "must not be empty"));
        }
        let inner = input.strip_prefix('*').unwrap_or(input);
        let inner = inner.strip_suffix('*').unwrap_or(inner);
        if inner.contains('*') {
            return Err(ValidationError::new(
                "target pattern",
                "`*` is only allowed at the start or end",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// Whether `target` matches.
    #[must_use]
    pub fn matches(&self, target: &str) -> bool {
        let text = self.0.as_str();
        let (leading, rest) = match text.strip_prefix('*') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (trailing, core) = match rest.strip_suffix('*') {
            Some(core) => (true, core),
            None => (false, rest),
        };
        match (leading, trailing) {
            (true, true) => target.contains(core),
            (true, false) => target.ends_with(core),
            (false, true) => target.starts_with(core),
            (false, false) => target == core,
        }
    }
}

impl fmt::Display for TargetPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TargetPattern {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<TargetPattern> for String {
    fn from(value: TargetPattern) -> Self {
        value.0
    }
}

/// A condition on one evidence value. A missing key never matches.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EvidencePredicate {
    /// Equal; integers and floats compare numerically.
    Equals {
        /// Evidence key.
        key: String,
        /// Expected value.
        value: EvidenceValue,
    },
    /// Numeric value at most `value`.
    LessOrEqual {
        /// Evidence key.
        key: String,
        /// Upper bound.
        value: f64,
    },
    /// Numeric value at least `value`.
    GreaterOrEqual {
        /// Evidence key.
        key: String,
        /// Lower bound.
        value: f64,
    },
    /// Text value contains `text`.
    Contains {
        /// Evidence key.
        key: String,
        /// Substring.
        text: String,
    },
}

fn as_number(value: &EvidenceValue) -> Option<f64> {
    match value {
        EvidenceValue::Int(i) => Some(*i as f64),
        EvidenceValue::Float(f) => Some(*f),
        EvidenceValue::Text(_) | EvidenceValue::Bool(_) => None,
    }
}

impl EvidencePredicate {
    /// Whether the signal's evidence satisfies this predicate.
    #[must_use]
    pub fn holds(&self, signal: &Signal) -> bool {
        let evidence = signal.evidence();
        match self {
            Self::Equals { key, value } => evidence.get(key).is_some_and(|actual| {
                match (as_number(actual), as_number(value)) {
                    (Some(a), Some(b)) => a == b,
                    _ => actual == value,
                }
            }),
            Self::LessOrEqual { key, value } => evidence
                .get(key)
                .and_then(as_number)
                .is_some_and(|n| n <= *value),
            Self::GreaterOrEqual { key, value } => evidence
                .get(key)
                .and_then(as_number)
                .is_some_and(|n| n >= *value),
            Self::Contains { key, text } => {
                matches!(evidence.get(key), Some(EvidenceValue::Text(t)) if t.contains(text.as_str()))
            }
        }
    }
}

/// A rule that marks matching incidents as known: they keep their status but do not notify.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AckRule {
    /// Unique id.
    pub id: AckRuleId,
    /// Restrict to one host; `None` matches every host.
    pub host: Option<HostId>,
    /// The check that must match exactly.
    pub check: CheckKey,
    /// Restrict targets; `None` matches any target, including none.
    pub target_pattern: Option<TargetPattern>,
    /// All must hold.
    pub predicates: Vec<EvidencePredicate>,
    /// Why the condition is acceptable.
    pub note: String,
    /// When the rule stops applying.
    pub expires_at: Option<Timestamp>,
}

impl AckRule {
    /// Whether this rule covers `signal` at `now`.
    #[must_use]
    pub fn matches(&self, signal: &Signal, now: Timestamp) -> bool {
        if self.expires_at.is_some_and(|at| now >= at) {
            return false;
        }
        if self.host.is_some_and(|host| host != signal.host()) {
            return false;
        }
        if &self.check != signal.check() {
            return false;
        }
        if let Some(pattern) = &self.target_pattern {
            let Some(target) = signal.target() else {
                return false;
            };
            if !pattern.matches(target.as_str()) {
                return false;
            }
        }
        self.predicates.iter().all(|p| p.holds(signal))
    }
}

/// A period when a host's signals are kept but notifications are held back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MaintenanceWindow {
    /// The host.
    pub host: HostId,
    /// Start, inclusive.
    pub starts_at: Timestamp,
    /// End, exclusive.
    pub ends_at: Timestamp,
    /// What is being done.
    pub note: String,
}

impl MaintenanceWindow {
    /// Whether `now` falls inside `[starts_at, ends_at)`.
    #[must_use]
    pub fn covers(&self, now: Timestamp) -> bool {
        self.starts_at <= now && now < self.ends_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::severity::Severity;
    use crate::signal::{Evidence, SourceKind, TargetKey};

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(1_700_000_000 + secs).unwrap()
    }

    fn signal(host: HostId, check: &str, target: Option<&str>) -> Signal {
        let mut evidence = Evidence::new();
        evidence.insert("reallocated", EvidenceValue::Int(8));
        evidence.insert("model", EvidenceValue::Text("WDC WD80EFAX".to_owned()));
        Signal::firing(
            SourceKind::Agent,
            host,
            CheckKey::parse(check).unwrap(),
            target.map(|t| TargetKey::parse(t).unwrap()),
            Severity::Warning,
            "disk reallocated sectors",
            at(0),
        )
        .with_evidence(evidence)
    }

    fn rule(host: Option<HostId>, pattern: Option<&str>) -> AckRule {
        AckRule {
            id: AckRuleId::new(),
            host,
            check: CheckKey::parse("disk.smart").unwrap(),
            target_pattern: pattern.map(|p| TargetPattern::parse(p).unwrap()),
            predicates: Vec::new(),
            note: "known old disk".to_owned(),
            expires_at: None,
        }
    }

    #[test]
    fn target_pattern_rejects_inner_wildcards() {
        for ok in ["sda", "sd*", "*sda", "*sd*", "*"] {
            assert!(TargetPattern::parse(ok).is_ok(), "{ok}");
        }
        for bad in ["", "s*a", "**a*", "a**b"] {
            assert!(TargetPattern::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ack_rule_matching_table() {
        let nas = HostId::new();
        let other = HostId::new();
        let sda = signal(nas, "disk.smart", Some("/dev/sda"));
        let cases: Vec<(&str, AckRule, &Signal, bool)> = vec![
            ("any host, no pattern", rule(None, None), &sda, true),
            ("same host", rule(Some(nas), None), &sda, true),
            ("other host", rule(Some(other), None), &sda, false),
            ("exact target", rule(None, Some("/dev/sda")), &sda, true),
            (
                "exact target differs",
                rule(None, Some("/dev/sdb")),
                &sda,
                false,
            ),
            ("prefix wildcard", rule(None, Some("/dev/sd*")), &sda, true),
            ("suffix wildcard", rule(None, Some("*sda")), &sda, true),
            ("both wildcards", rule(None, Some("*dev*")), &sda, true),
            ("star matches any", rule(None, Some("*")), &sda, true),
            (
                "prefix does not match",
                rule(None, Some("/dev/nvme*")),
                &sda,
                false,
            ),
        ];
        for (name, rule, signal, expected) in cases {
            assert_eq!(rule.matches(signal, at(10)), expected, "{name}");
        }

        let no_target = signal(nas, "disk.smart", None);
        assert!(
            !rule(None, Some("*")).matches(&no_target, at(10)),
            "pattern needs a target"
        );
        assert!(rule(None, None).matches(&no_target, at(10)));

        let other_check = signal(nas, "disk.free", Some("/dev/sda"));
        assert!(!rule(None, None).matches(&other_check, at(10)));
    }

    #[test]
    fn ack_rule_expiry_and_predicates() {
        let nas = HostId::new();
        let sda = signal(nas, "disk.smart", Some("/dev/sda"));
        let mut expiring = rule(None, None);
        expiring.expires_at = Some(at(100));
        assert!(expiring.matches(&sda, at(99)));
        assert!(!expiring.matches(&sda, at(100)));

        let with = |predicates: Vec<EvidencePredicate>| {
            let mut r = rule(None, None);
            r.predicates = predicates;
            r.matches(&sda, at(10))
        };
        let key = || "reallocated".to_owned();
        assert!(with(vec![EvidencePredicate::LessOrEqual {
            key: key(),
            value: 8.0
        }]));
        assert!(!with(vec![EvidencePredicate::LessOrEqual {
            key: key(),
            value: 7.0
        }]));
        assert!(with(vec![EvidencePredicate::GreaterOrEqual {
            key: key(),
            value: 8.0
        }]));
        assert!(with(vec![EvidencePredicate::Equals {
            key: key(),
            value: EvidenceValue::Float(8.0)
        }]));
        assert!(with(vec![EvidencePredicate::Contains {
            key: "model".to_owned(),
            text: "WD80".to_owned()
        }]));
        assert!(!with(vec![EvidencePredicate::Contains {
            key: "missing".to_owned(),
            text: "x".to_owned()
        }]));
        assert!(!with(vec![
            EvidencePredicate::LessOrEqual {
                key: key(),
                value: 8.0
            },
            EvidencePredicate::GreaterOrEqual {
                key: key(),
                value: 9.0
            },
        ]));
    }

    #[test]
    fn maintenance_window_is_half_open() {
        let window = MaintenanceWindow {
            host: HostId::new(),
            starts_at: at(0),
            ends_at: at(60),
            note: "swap disk".to_owned(),
        };
        assert!(!window.covers(at(-1)));
        assert!(window.covers(at(0)));
        assert!(window.covers(at(59)));
        assert!(!window.covers(at(60)));
    }
}
