//! What one check run found.

use domovoi_core::{
    DigestFact, Evidence, EvidenceValue, FactValue, HostId, Severity, SignalClass, TargetKey,
};
use jiff::Timestamp;

/// The result of one run: observations to turn into signals and facts for the digest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CheckReport {
    /// One entry per target the check looked at.
    pub observations: Vec<Observation>,
    /// Measurements for the daily digest.
    pub facts: Vec<DigestFact>,
}

/// The state of one target right now.
///
/// `firing: false` means the target is fine, so an open incident for it can resolve.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// What the observation is about, such as a mount point; `None` for the whole host.
    pub target: Option<TargetKey>,
    /// Whether the condition holds.
    pub firing: bool,
    /// How serious it is. `Info` for observations that are not firing.
    pub severity: Severity,
    /// Overrides the check's class, for example an immediate power loss.
    pub class: Option<SignalClass>,
    /// One-line description.
    pub summary: String,
    /// Measurements and details. Volatile values belong here, never in the target.
    pub evidence: Evidence,
}

impl Observation {
    /// A firing observation.
    #[must_use]
    pub fn firing(
        target: Option<TargetKey>,
        severity: Severity,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            target,
            firing: true,
            severity,
            class: None,
            summary: summary.into(),
            evidence: Evidence::new(),
        }
    }

    /// An observation that the target is fine.
    #[must_use]
    pub fn ok(target: Option<TargetKey>, summary: impl Into<String>) -> Self {
        Self {
            target,
            firing: false,
            severity: Severity::Info,
            class: None,
            summary: summary.into(),
            evidence: Evidence::new(),
        }
    }

    /// Adds one evidence entry.
    #[must_use]
    pub fn with(mut self, key: &str, value: EvidenceValue) -> Self {
        self.evidence.insert(key, value);
        self
    }

    /// Adds a numeric evidence entry, rounded to two decimals.
    #[must_use]
    pub fn with_number(self, key: &str, value: f64) -> Self {
        self.with(key, EvidenceValue::Float(round2(value)))
    }

    /// Adds a text evidence entry.
    #[must_use]
    pub fn with_text(self, key: &str, value: impl Into<String>) -> Self {
        self.with(key, EvidenceValue::Text(value.into()))
    }

    /// Marks the observation as immediate: it skips confirmation runs.
    #[must_use]
    pub fn immediate(mut self) -> Self {
        self.class = Some(SignalClass::Immediate);
        self
    }
}

/// Rounds to two decimals so evidence and facts stay readable.
#[must_use]
pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// A numeric digest fact, rounded to two decimals.
#[must_use]
pub fn number_fact(
    host: HostId,
    key: impl Into<String>,
    label: impl Into<String>,
    value: f64,
    unit: &str,
    observed_at: Timestamp,
) -> DigestFact {
    DigestFact {
        host,
        key: key.into(),
        label: label.into(),
        value: FactValue::Number(round2(value)),
        unit: Some(unit.to_owned()),
        observed_at,
    }
}

/// A text digest fact.
#[must_use]
pub fn text_fact(
    host: HostId,
    key: impl Into<String>,
    label: impl Into<String>,
    value: impl Into<String>,
    observed_at: Timestamp,
) -> DigestFact {
    DigestFact {
        host,
        key: key.into(),
        label: label.into(),
        value: FactValue::Text(value.into()),
        unit: None,
        observed_at,
    }
}
