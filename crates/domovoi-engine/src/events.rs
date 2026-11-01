//! Domain events: what changed, published on the broadcast bus after it is stored.

use std::collections::BTreeMap;

use domovoi_core::{ActionId, ActionStatus, Incident, Severity, SignalId, TriageOutcome, UserId};
use jiff::Timestamp;

/// What happened to an incident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// A new incident was confirmed.
    IncidentOpened,
    /// Another firing signal was folded into an active incident.
    IncidentFolded {
        /// The folded signal.
        signal: SignalId,
        /// Firing signals folded so far.
        occurrences: u32,
    },
    /// An active incident's severity rose.
    IncidentEscalated {
        /// The severity before.
        from: Severity,
        /// The severity now.
        to: Severity,
    },
    /// A resolved incident fired again within the reopen window.
    IncidentReopened,
    /// The incident resolved.
    IncidentResolved,
    /// A human acknowledged it.
    IncidentAcked {
        /// Who.
        by: UserId,
    },
    /// A human muted it.
    IncidentMuted {
        /// Until when.
        until: Timestamp,
    },
    /// A mute ended, early or on time.
    IncidentUnmuted,
    /// The incident needs (fresh) triage: it opened, reopened or escalated.
    TriageRequested,
    /// Triage finished, was skipped or failed; the card shows the outcome.
    TriageCompleted {
        /// What triage produced.
        outcome: TriageOutcome,
        /// Plain chat tokens of the actions triage proposed, for the card's buttons. Only
        /// their hashes are stored.
        tokens: BTreeMap<ActionId, String>,
    },
    /// Triage failed several times in a row; incidents go out without triage.
    TriageDegraded {
        /// Why, in one line.
        reason: String,
    },
    /// A human approved an action; the executor picks it up.
    ActionApproved {
        /// Which action.
        action: ActionId,
    },
    /// An action changed status.
    ActionChanged {
        /// Which action.
        action: ActionId,
        /// Its status now.
        status: ActionStatus,
    },
    /// The loop guard stopped an approved action from running again.
    ActionBlocked {
        /// Which action.
        action: ActionId,
        /// Why, in one line.
        reason: String,
    },
}

/// One change to one incident, broadcast to every consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainEvent {
    /// What happened.
    pub kind: EventKind,
    /// The incident after the change.
    pub incident: Incident,
    /// Whether to tell people; `false` while a maintenance window or an ack rule applies.
    /// The incident changes state either way.
    pub notify: bool,
}

impl DomainEvent {
    /// An event of `kind` for `incident`.
    #[must_use]
    pub fn new(kind: EventKind, incident: &Incident, notify: bool) -> Self {
        Self {
            kind,
            incident: incident.clone(),
            notify,
        }
    }
}
