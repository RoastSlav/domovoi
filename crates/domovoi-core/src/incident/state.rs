//! Incident data: status, policy and the incident record itself.

use jiff::{SignedDuration, Timestamp};

use crate::fingerprint::Fingerprint;
use crate::history::EntityHistory;
use crate::id::{HostId, IncidentId};
use crate::severity::Severity;
use crate::signal::{CheckKey, Signal, TargetKey};
use crate::verdict::Verdict;

/// Where an incident is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IncidentStatus {
    /// Firing but not yet confirmed by enough consecutive signals.
    Pending,
    /// Confirmed and unhandled.
    Open,
    /// A human has seen it.
    Acked,
    /// Silenced until the given time.
    Muted {
        /// When the mute ends.
        until: Timestamp,
    },
    /// The condition cleared.
    Resolved,
}

/// Tunable behaviour of the incident state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IncidentPolicy {
    /// Consecutive firing signals needed to confirm a normal-class incident.
    pub confirm_runs: u8,
    /// How long after resolving the same fingerprint reopens the incident instead of starting a new one.
    pub reopen_window: SignedDuration,
    /// Resolve incidents that stop firing for this long; `None` means only an explicit resolve ends them.
    pub ttl: Option<SignedDuration>,
}

impl Default for IncidentPolicy {
    fn default() -> Self {
        Self {
            confirm_runs: 2,
            reopen_window: SignedDuration::from_hours(6),
            ttl: None,
        }
    }
}

/// One tracked problem on one host.
///
/// State fields are read through getters. Only [`Incident::new_pending`] and
/// [`apply`](super::apply) change an incident, so every status change goes through the
/// state machine.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Incident {
    pub(super) id: IncidentId,
    pub(super) fingerprint: Fingerprint,
    pub(super) host: HostId,
    pub(super) check: CheckKey,
    pub(super) target: Option<TargetKey>,
    pub(super) title: String,
    pub(super) status: IncidentStatus,
    pub(super) severity: Severity,
    pub(super) verdict: Option<Verdict>,
    pub(super) opened_at: Timestamp,
    pub(super) confirmed_at: Option<Timestamp>,
    pub(super) resolved_at: Option<Timestamp>,
    pub(super) last_firing_at: Timestamp,
    pub(super) firing_streak: u8,
    pub(super) occurrences: u32,
    pub(super) history: EntityHistory,
}

impl Incident {
    /// Builds the initial pending incident for the first firing `signal`.
    ///
    /// The incident starts with a zero streak and zero occurrences. The caller then applies
    /// the same signal as an [`IncidentEvent::Firing`](super::IncidentEvent::Firing) so the
    /// first firing is counted once and `Immediate` signals open at once.
    #[must_use]
    pub fn new_pending(signal: &Signal, now: Timestamp) -> Self {
        Self {
            id: IncidentId::new(),
            fingerprint: signal.fingerprint(),
            host: signal.host(),
            check: signal.check().clone(),
            target: signal.target().cloned(),
            title: signal.summary().to_owned(),
            status: IncidentStatus::Pending,
            severity: signal.severity(),
            verdict: None,
            opened_at: now,
            confirmed_at: None,
            resolved_at: None,
            last_firing_at: signal.observed_at(),
            firing_streak: 0,
            occurrences: 0,
            history: EntityHistory::new(),
        }
    }

    /// Replaces the generated id, for rehydrating stored incidents and fixed test ids.
    #[must_use]
    pub fn with_id(mut self, id: IncidentId) -> Self {
        self.id = id;
        self
    }

    /// Sets the triage verdict. Triage output does not change the incident status.
    pub fn set_verdict(&mut self, verdict: Option<Verdict>) {
        self.verdict = verdict;
    }

    /// Unique id.
    #[must_use]
    pub fn id(&self) -> IncidentId {
        self.id
    }

    /// Identity of the problem: host, check and target.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// The affected host.
    #[must_use]
    pub fn host(&self) -> HostId {
        self.host
    }

    /// The check that fires.
    #[must_use]
    pub fn check(&self) -> &CheckKey {
        &self.check
    }

    /// What the check looks at.
    #[must_use]
    pub fn target(&self) -> Option<&TargetKey> {
        self.target.as_ref()
    }

    /// Short title taken from the first signal summary.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Current status.
    #[must_use]
    pub fn status(&self) -> IncidentStatus {
        self.status
    }

    /// Highest severity seen while firing.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Triage verdict, if any.
    #[must_use]
    pub fn verdict(&self) -> Option<Verdict> {
        self.verdict
    }

    /// When the first signal arrived.
    #[must_use]
    pub fn opened_at(&self) -> Timestamp {
        self.opened_at
    }

    /// When the incident was confirmed.
    #[must_use]
    pub fn confirmed_at(&self) -> Option<Timestamp> {
        self.confirmed_at
    }

    /// When the incident last resolved; `Some` exactly while resolved.
    #[must_use]
    pub fn resolved_at(&self) -> Option<Timestamp> {
        self.resolved_at
    }

    /// When a firing signal last arrived.
    #[must_use]
    pub fn last_firing_at(&self) -> Timestamp {
        self.last_firing_at
    }

    /// Consecutive firing signals while pending.
    #[must_use]
    pub fn firing_streak(&self) -> u8 {
        self.firing_streak
    }

    /// Firing signals folded into this incident.
    #[must_use]
    pub fn occurrences(&self) -> u32 {
        self.occurrences
    }

    /// Recorded values of the tracked measurement.
    #[must_use]
    pub fn history(&self) -> &EntityHistory {
        &self.history
    }
}
