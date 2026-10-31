//! Signals: single observations from a check, a webhook or a probe.

mod evidence;
mod keys;

pub use evidence::{Evidence, EvidenceValue};
pub use keys::{CheckKey, TargetKey};

use jiff::Timestamp;

use crate::fingerprint::Fingerprint;
use crate::id::{HostId, SignalId};
use crate::severity::Severity;

/// Maximum characters in a signal summary, including the truncation marker.
pub const SUMMARY_MAX_CHARS: usize = 300;

/// Where a signal came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceKind {
    /// A check built into Domovoi.
    BuiltIn,
    /// An external system posting to a webhook.
    Webhook {
        /// Which kind of webhook, for example `uptime-kuma`.
        kind: String,
    },
    /// A network probe run by the hub.
    Probe,
    /// A check run by a remote agent.
    Agent,
}

/// Whether the condition is currently present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalStatus {
    /// The condition is present.
    Firing,
    /// The condition has cleared.
    Resolved,
}

/// Whether a signal must skip confirmation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalClass {
    /// Needs the configured number of consecutive firings.
    Normal,
    /// Opens an incident at once, for example power loss.
    Immediate,
}

/// One observation of a condition on a host.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Signal {
    id: SignalId,
    source: SourceKind,
    host: HostId,
    check: CheckKey,
    target: Option<TargetKey>,
    status: SignalStatus,
    severity: Severity,
    class: SignalClass,
    summary: String,
    evidence: Evidence,
    observed_at: Timestamp,
}

fn clamp_summary(summary: impl Into<String>) -> String {
    let summary = summary.into();
    if summary.chars().count() <= SUMMARY_MAX_CHARS {
        return summary;
    }
    let mut cut: String = summary.chars().take(SUMMARY_MAX_CHARS - 1).collect();
    cut.push('\u{2026}');
    cut
}

impl Signal {
    /// A firing signal. The summary is truncated with an ellipsis past [`SUMMARY_MAX_CHARS`].
    #[must_use]
    pub fn firing(
        source: SourceKind,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        severity: Severity,
        summary: impl Into<String>,
        observed_at: Timestamp,
    ) -> Self {
        Self {
            id: SignalId::new(),
            source,
            host,
            check,
            target,
            status: SignalStatus::Firing,
            severity,
            class: SignalClass::Normal,
            summary: clamp_summary(summary),
            evidence: Evidence::new(),
            observed_at,
        }
    }

    /// A resolved signal. Its severity is `Info`.
    #[must_use]
    pub fn resolved(
        source: SourceKind,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        summary: impl Into<String>,
        observed_at: Timestamp,
    ) -> Self {
        Self {
            status: SignalStatus::Resolved,
            ..Self::firing(
                source,
                host,
                check,
                target,
                Severity::Info,
                summary,
                observed_at,
            )
        }
    }

    /// Sets the class, for example [`SignalClass::Immediate`].
    #[must_use]
    pub fn with_class(mut self, class: SignalClass) -> Self {
        self.class = class;
        self
    }

    /// Replaces the evidence.
    #[must_use]
    pub fn with_evidence(mut self, evidence: Evidence) -> Self {
        self.evidence = evidence;
        self
    }

    /// Replaces the generated id, for rehydrating stored signals and fixed test ids.
    #[must_use]
    pub fn with_id(mut self, id: SignalId) -> Self {
        self.id = id;
        self
    }

    /// Unique id of this observation.
    #[must_use]
    pub fn id(&self) -> SignalId {
        self.id
    }

    /// Where the signal came from.
    #[must_use]
    pub fn source(&self) -> &SourceKind {
        &self.source
    }

    /// The host observed.
    #[must_use]
    pub fn host(&self) -> HostId {
        self.host
    }

    /// The check that produced the signal.
    #[must_use]
    pub fn check(&self) -> &CheckKey {
        &self.check
    }

    /// What the check looked at, if anything specific.
    #[must_use]
    pub fn target(&self) -> Option<&TargetKey> {
        self.target.as_ref()
    }

    /// Firing or resolved.
    #[must_use]
    pub fn status(&self) -> SignalStatus {
        self.status
    }

    /// How serious the condition is.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Normal or immediate.
    #[must_use]
    pub fn class(&self) -> SignalClass {
        self.class
    }

    /// Short human-readable description, at most [`SUMMARY_MAX_CHARS`] characters.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Measurements and context.
    #[must_use]
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// When the check observed the condition.
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// The fingerprint of host, check and target.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(self.host, &self.check, self.target.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(summary: &str) -> Signal {
        Signal::firing(
            SourceKind::BuiltIn,
            HostId::new(),
            CheckKey::parse("disk.free").unwrap(),
            None,
            Severity::Warning,
            summary,
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn long_summary_is_truncated_with_ellipsis() {
        let s = sample(&"x".repeat(500));
        assert_eq!(s.summary().chars().count(), SUMMARY_MAX_CHARS);
        assert!(s.summary().ends_with('\u{2026}'));
    }

    #[test]
    fn short_summary_is_kept() {
        assert_eq!(sample("disk almost full").summary(), "disk almost full");
    }

    #[test]
    fn resolved_signal_has_resolved_status() {
        let s = Signal::resolved(
            SourceKind::Agent,
            HostId::new(),
            CheckKey::parse("disk.free").unwrap(),
            None,
            "ok",
            Timestamp::UNIX_EPOCH,
        );
        assert_eq!(s.status(), SignalStatus::Resolved);
    }
}
