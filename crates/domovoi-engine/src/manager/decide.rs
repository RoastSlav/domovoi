//! The pure decision function: given what is known and one command, what to store and
//! what to publish. No I/O, no clock, no async (ADR 0011).

use std::collections::BTreeMap;

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::{
    AckRule, Fingerprint, Incident, IncidentChange, IncidentEvent, IncidentId, IncidentStatus,
    MaintenanceWindow, Signal, SignalStatus, TransitionError, apply,
};
use jiff::Timestamp;
use serde_json::json;

use super::command::{Command, CommandError};
use crate::correlate::{Candidates, Target, correlate};
use crate::events::{DomainEvent, EventKind};
use crate::policy::EnginePolicy;
use crate::suppress::{in_maintenance, suppression};

/// What the manager knows when it decides.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// Every active incident.
    pub active: Vec<Incident>,
    /// The most recently resolved incident per fingerprint, for reopening.
    pub resolved: BTreeMap<Fingerprint, Incident>,
    /// Every ack rule.
    pub acks: Vec<AckRule>,
    /// Maintenance windows of the hosts involved.
    pub windows: Vec<MaintenanceWindow>,
    /// For [`Command::AttachTriage`]: the incident, loaded even when resolved.
    pub triaged: Option<Incident>,
}

/// One store write, applied in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Write {
    /// Store a signal, linked to an incident if it belongs to one.
    InsertSignal(Signal, Option<IncidentId>),
    /// Store a new incident.
    InsertIncident(Incident),
    /// Replace a stored incident.
    UpdateIncident(Incident),
    /// Append to an incident's timeline.
    Timeline(IncidentTimelineEntry),
    /// Delete a pending incident that resolved before confirmation.
    DiscardPending(IncidentId),
}

/// The outcome of one command.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decision {
    /// Store these, in order.
    pub writes: Vec<Write>,
    /// Then publish these, in order.
    pub events: Vec<DomainEvent>,
    /// Pending incidents dropped before confirmation.
    pub flaps: u32,
}

fn status_name(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Pending => "pending",
        IncidentStatus::Open => "open",
        IncidentStatus::Acked => "acked",
        IncidentStatus::Muted { .. } => "muted",
        IncidentStatus::Resolved => "resolved",
    }
}

fn not_allowed(err: TransitionError) -> CommandError {
    CommandError::NotAllowed(err.to_string())
}

/// Accumulates writes and events while one command is decided.
struct Builder<'a> {
    state: State,
    policy: &'a EnginePolicy,
    now: Timestamp,
    out: Decision,
}

impl Builder<'_> {
    fn timeline(
        &mut self,
        incident: &Incident,
        kind: &str,
        summary: String,
        detail: serde_json::Value,
    ) {
        self.out.writes.push(Write::Timeline(IncidentTimelineEntry {
            incident: incident.id(),
            at: self.now,
            kind: kind.to_owned(),
            summary,
            detail,
        }));
    }

    fn event(&mut self, kind: EventKind, incident: &Incident, notify: bool) {
        self.out
            .events
            .push(DomainEvent::new(kind, incident, notify));
    }

    /// Turns the state machine's changes into timeline entries and events.
    fn record(
        &mut self,
        incident: &Incident,
        changes: &[IncidentChange],
        signal: Option<&Signal>,
        notify: bool,
    ) {
        let confirmed = changes.contains(&IncidentChange::Confirmed);
        let reopened = changes.contains(&IncidentChange::Reopened);
        for change in changes {
            match change {
                IncidentChange::Confirmed => {
                    self.event(EventKind::IncidentOpened, incident, notify);
                    self.event(EventKind::TriageRequested, incident, notify);
                }
                IncidentChange::Reopened => {
                    self.event(EventKind::IncidentReopened, incident, notify);
                    self.event(EventKind::TriageRequested, incident, notify);
                }
                IncidentChange::Folded { occurrences } => {
                    if let Some(signal) = signal {
                        let kind = EventKind::IncidentFolded {
                            signal: signal.id(),
                            occurrences: *occurrences,
                        };
                        self.event(kind, incident, notify);
                    }
                }
                IncidentChange::SeverityChanged { from, to } => {
                    let still_pending = incident.status() == IncidentStatus::Pending;
                    if !still_pending && !confirmed && !reopened {
                        let kind = EventKind::IncidentEscalated {
                            from: *from,
                            to: *to,
                        };
                        self.event(kind, incident, notify);
                        self.event(EventKind::TriageRequested, incident, notify);
                    }
                    self.timeline(
                        incident,
                        "severity.changed",
                        format!("{from:?} → {to:?}").to_lowercase(),
                        json!({ "from": from, "to": to }),
                    );
                }
                IncidentChange::StatusChanged { from, to } => {
                    self.timeline(
                        incident,
                        "status.changed",
                        format!("{} → {}", status_name(*from), status_name(*to)),
                        json!({ "from": from, "to": to }),
                    );
                    match (from, to) {
                        (_, IncidentStatus::Resolved) => {
                            self.event(EventKind::IncidentResolved, incident, notify);
                        }
                        (IncidentStatus::Muted { .. }, IncidentStatus::Open) => {
                            self.event(EventKind::IncidentUnmuted, incident, notify);
                        }
                        _ => {}
                    }
                }
                IncidentChange::Dropped | IncidentChange::ValueRecorded { .. } => {}
            }
        }
    }

    fn notify_for(&self, signal: &Signal, incident: &Incident) -> bool {
        suppression(
            signal,
            incident,
            &self.state.acks,
            &self.state.windows,
            self.now,
        )
        .is_none()
    }

    fn firing_event(signal: &Signal) -> IncidentEvent {
        IncidentEvent::Firing {
            severity: signal.severity(),
            class: signal.class(),
            observed_at: signal.observed_at(),
            value: signal.evidence().tracked_value(),
        }
    }

    fn ingest(&mut self, signal: &Signal) {
        let fingerprint = signal.fingerprint();
        let target = {
            let candidates = Candidates {
                active: &self.state.active,
                recent_resolved: self.state.resolved.get(&fingerprint),
            };
            correlate(signal, &candidates, self.policy, self.now)
        };
        match target {
            Target::Ignore => {
                self.out
                    .writes
                    .push(Write::InsertSignal(signal.clone(), None));
            }
            Target::New => self.open_new(signal),
            Target::Existing(id) => self.fold_into(signal, id),
            Target::Reopen(id) => self.reopen(signal, id),
        }
    }

    fn open_new(&mut self, signal: &Signal) {
        let mut incident = Incident::new_pending(signal, self.now);
        let changes = match apply(
            &mut incident,
            Self::firing_event(signal),
            &self.policy.incident,
            self.now,
        ) {
            Ok(changes) => changes,
            Err(err) => {
                tracing::warn!(%err, "a new incident rejected its first signal");
                self.out
                    .writes
                    .push(Write::InsertSignal(signal.clone(), None));
                return;
            }
        };
        let notify = self.notify_for(signal, &incident);
        self.out
            .writes
            .push(Write::InsertIncident(incident.clone()));
        self.out
            .writes
            .push(Write::InsertSignal(signal.clone(), Some(incident.id())));
        self.timeline(
            &incident,
            "incident.created",
            signal.summary().to_owned(),
            json!({ "signal": signal.id() }),
        );
        self.record(&incident, &changes, Some(signal), notify);
        self.state.active.push(incident);
    }

    fn fold_into(&mut self, signal: &Signal, id: IncidentId) {
        let Some(pos) = self.state.active.iter().position(|i| i.id() == id) else {
            self.out
                .writes
                .push(Write::InsertSignal(signal.clone(), None));
            return;
        };
        let mut incident = self.state.active[pos].clone();
        let fingerprint = signal.fingerprint();
        let firing = signal.status() == SignalStatus::Firing;
        if !incident.is_member(&fingerprint) {
            incident.relate(fingerprint, firing);
            self.timeline(
                &incident,
                "incident.grouped",
                format!("grouped {}", signal.check()),
                json!({ "fingerprint": fingerprint, "check": signal.check(), "source": signal.source() }),
            );
        }
        let still_firing = incident.mark_member(fingerprint, firing);
        let event = if firing {
            Self::firing_event(signal)
        } else if still_firing {
            // Another member of this group still fires: keep the incident open.
            self.out
                .writes
                .push(Write::UpdateIncident(incident.clone()));
            self.out
                .writes
                .push(Write::InsertSignal(signal.clone(), Some(id)));
            self.state.active[pos] = incident;
            return;
        } else {
            IncidentEvent::Resolved {
                observed_at: signal.observed_at(),
            }
        };
        let changes = match apply(&mut incident, event, &self.policy.incident, self.now) {
            Ok(changes) => changes,
            Err(err) => {
                tracing::warn!(%err, incident = %id, "a signal did not apply");
                self.out
                    .writes
                    .push(Write::InsertSignal(signal.clone(), Some(id)));
                return;
            }
        };
        if changes.contains(&IncidentChange::Dropped) {
            self.out
                .writes
                .push(Write::InsertSignal(signal.clone(), None));
            self.out.writes.push(Write::DiscardPending(id));
            self.out.flaps += 1;
            self.state.active.remove(pos);
            return;
        }
        let notify = self.notify_for(signal, &incident);
        self.out
            .writes
            .push(Write::UpdateIncident(incident.clone()));
        self.out
            .writes
            .push(Write::InsertSignal(signal.clone(), Some(id)));
        self.record(&incident, &changes, Some(signal), notify);
        if incident.status() == IncidentStatus::Resolved {
            self.state.active.remove(pos);
            self.state.resolved.insert(incident.fingerprint(), incident);
        } else {
            self.state.active[pos] = incident;
        }
    }

    fn reopen(&mut self, signal: &Signal, id: IncidentId) {
        let fingerprint = signal.fingerprint();
        let Some(mut incident) = self.state.resolved.get(&fingerprint).cloned() else {
            self.open_new(signal);
            return;
        };
        let changes = match apply(
            &mut incident,
            Self::firing_event(signal),
            &self.policy.incident,
            self.now,
        ) {
            Ok(changes) => changes,
            // Past the window after all: a new incident.
            Err(TransitionError::ReopenWindowElapsed) => {
                self.state.resolved.remove(&fingerprint);
                self.open_new(signal);
                return;
            }
            Err(err) => {
                tracing::warn!(%err, incident = %id, "a reopening signal did not apply");
                self.out
                    .writes
                    .push(Write::InsertSignal(signal.clone(), None));
                return;
            }
        };
        incident.mark_member(fingerprint, true);
        let notify = self.notify_for(signal, &incident);
        self.out
            .writes
            .push(Write::UpdateIncident(incident.clone()));
        self.out
            .writes
            .push(Write::InsertSignal(signal.clone(), Some(id)));
        self.record(&incident, &changes, Some(signal), notify);
        self.state.resolved.remove(&fingerprint);
        self.state.active.push(incident);
    }

    /// Applies a human command to one active incident.
    fn human(
        &mut self,
        id: IncidentId,
        event: IncidentEvent,
        kind: EventKind,
        by: &str,
    ) -> Result<(), CommandError> {
        let pos = self
            .state
            .active
            .iter()
            .position(|i| i.id() == id)
            .ok_or(CommandError::UnknownIncident(id))?;
        let mut incident = self.state.active[pos].clone();
        let changes =
            apply(&mut incident, event, &self.policy.incident, self.now).map_err(not_allowed)?;
        self.out
            .writes
            .push(Write::UpdateIncident(incident.clone()));
        self.timeline(
            &incident,
            "human.action",
            by.to_owned(),
            serde_json::Value::Null,
        );
        self.record(&incident, &changes, None, true);
        if !matches!(kind, EventKind::IncidentUnmuted) {
            self.event(kind, &incident, true);
        }
        self.state.active[pos] = incident;
        Ok(())
    }

    fn tick(&mut self) {
        let mut still_active = Vec::with_capacity(self.state.active.len());
        for mut incident in std::mem::take(&mut self.state.active) {
            let changes = match apply(
                &mut incident,
                IncidentEvent::Tick,
                &self.policy.incident,
                self.now,
            ) {
                Ok(changes) => changes,
                Err(err) => {
                    tracing::warn!(%err, incident = %incident.id(), "tick did not apply");
                    still_active.push(incident);
                    continue;
                }
            };
            if !changes.is_empty() {
                let notify = !in_maintenance(incident.host(), &self.state.windows, self.now);
                self.out
                    .writes
                    .push(Write::UpdateIncident(incident.clone()));
                self.record(&incident, &changes, None, notify);
            }
            if incident.status() == IncidentStatus::Resolved {
                self.state.resolved.insert(incident.fingerprint(), incident);
            } else {
                still_active.push(incident);
            }
        }
        self.state.active = still_active;
    }
}

/// Decides what `command` does to `state` at `now`: the writes to store and the events to
/// publish once they are stored. An error means nothing is written.
pub fn decide(
    state: &State,
    command: &Command,
    policy: &EnginePolicy,
    now: Timestamp,
) -> Result<Decision, CommandError> {
    let mut builder = Builder {
        state: state.clone(),
        policy,
        now,
        out: Decision::default(),
    };
    match command {
        Command::Ingest(signals) => {
            for signal in signals {
                builder.ingest(signal);
            }
        }
        Command::Ack { incident, by } => builder.human(
            *incident,
            IncidentEvent::Ack { by: *by },
            EventKind::IncidentAcked { by: *by },
            &format!("acked by user:{by}"),
        )?,
        Command::Mute {
            incident,
            until,
            by,
        } => builder.human(
            *incident,
            IncidentEvent::Mute { until: *until },
            EventKind::IncidentMuted { until: *until },
            &format!("muted by user:{by} until {until}"),
        )?,
        Command::Unmute { incident } => builder.human(
            *incident,
            IncidentEvent::Unmute,
            EventKind::IncidentUnmuted,
            "unmuted",
        )?,
        Command::Tick => builder.tick(),
        Command::AttachTriage {
            incident,
            outcome,
            tokens,
        } => return super::triage::attach(state, *incident, outcome, tokens, now),
        Command::RecordFollowUp {
            incident,
            asked_by,
            question,
            answer,
        } => {
            return Ok(super::triage::follow_up(
                *incident, *asked_by, question, answer, now,
            ));
        }
    }
    Ok(builder.out)
}
