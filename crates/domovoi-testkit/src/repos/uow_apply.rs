//! Applying the typed state writes of a transaction to in-memory state.

use domovoi_core::ports::{
    ActionGuard, AggregateRef, CommitError, CommitReceipt, Expectation, Expected, JournalDraft,
    Observed, StateWrite, timeline_journal,
};
use domovoi_core::{ActionRequest, Incident, IncidentStatus, Note};

use super::actions::token_taken;
use super::incidents::second_active;
use super::{State, set_version, version};

fn conflict(aggregate: AggregateRef, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate,
        expected,
        found,
    }
}

/// Stores one state write, updating `receipt` and queueing mirrored journal rows.
pub(super) fn apply(
    state: &mut State,
    write: &StateWrite,
    receipt: &mut CommitReceipt,
    mirrors: &mut Vec<JournalDraft>,
) -> Result<(), CommitError> {
    match write {
        StateWrite::Signal { signal, incident } => {
            if state.signals.contains_key(&signal.id()) {
                return Err(CommitError::Constraint("signal id exists".to_owned()));
            }
            state
                .signals
                .insert(signal.id(), (signal.clone(), *incident));
            Ok(())
        }
        StateWrite::Incident { incident, expected } => {
            apply_incident(state, incident, *expected, receipt)
        }
        StateWrite::IncidentEvent(entry) => {
            if !state.incidents.contains_key(&entry.incident) {
                return Err(CommitError::Constraint("unknown incident".to_owned()));
            }
            state
                .timeline
                .entry(entry.incident)
                .or_default()
                .push(entry.clone());
            mirrors.push(timeline_journal(entry));
            Ok(())
        }
        StateWrite::DiscardPendingIncident { id, expected } => apply_discard(state, *id, *expected),
        StateWrite::Action { action, expected } => apply_action(state, action, expected),
        StateWrite::Note { note, expected } => apply_note(state, note, *expected, receipt),
    }
}

fn apply_incident(
    state: &mut State,
    incident: &Incident,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::incident(incident.id());
    let exists = state.incidents.contains_key(&incident.id());
    match expected {
        Expected::New if exists => {
            let stored = version(state, &aggregate);
            Err(conflict(
                aggregate,
                Expectation::New,
                Observed::Version(stored),
            ))
        }
        Expected::New => {
            if second_active(state, incident) {
                return Err(CommitError::Constraint(
                    "an active incident with this fingerprint exists".to_owned(),
                ));
            }
            state.incidents.insert(incident.id(), incident.clone());
            set_version(state, aggregate.clone(), 1);
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        Expected::Version(n) if !exists => Err(conflict(
            aggregate,
            Expectation::Version(n),
            Observed::Missing,
        )),
        Expected::Version(n) => {
            let stored = version(state, &aggregate);
            if stored != n {
                return Err(conflict(
                    aggregate,
                    Expectation::Version(n),
                    Observed::Version(stored),
                ));
            }
            if second_active(state, incident) {
                return Err(CommitError::Constraint(
                    "an active incident with this fingerprint exists".to_owned(),
                ));
            }
            state.incidents.insert(incident.id(), incident.clone());
            set_version(state, aggregate.clone(), n + 1);
            receipt.versions.push((aggregate, n + 1));
            Ok(())
        }
    }
}

fn apply_discard(
    state: &mut State,
    id: domovoi_core::IncidentId,
    expected: u64,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::incident(id);
    let Some(incident) = state.incidents.get(&id) else {
        return Err(conflict(
            aggregate,
            Expectation::Version(expected),
            Observed::Missing,
        ));
    };
    let stored = version(state, &aggregate);
    if stored != expected {
        return Err(conflict(
            aggregate,
            Expectation::Version(expected),
            Observed::Version(stored),
        ));
    }
    if incident.status() != IncidentStatus::Pending {
        return Err(CommitError::Constraint(
            "only pending incidents are discarded".to_owned(),
        ));
    }
    state.incidents.remove(&id);
    state.versions.remove(&aggregate);
    state.timeline.remove(&id);
    for (_, link) in state.signals.values_mut() {
        if *link == Some(id) {
            *link = None;
        }
    }
    Ok(())
}

fn apply_action(
    state: &mut State,
    action: &ActionRequest,
    expected: &ActionGuard,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::action(action.id());
    let stored = state.actions.get(&action.id()).map(ActionRequest::status);
    match expected {
        ActionGuard::New => {
            if let Some(status) = stored {
                return Err(conflict(
                    aggregate,
                    Expectation::New,
                    Observed::Status(status.name().to_owned()),
                ));
            }
        }
        ActionGuard::Status(name) => {
            let Some(status) = stored else {
                return Err(conflict(
                    aggregate,
                    Expectation::Status(name.clone()),
                    Observed::Missing,
                ));
            };
            if status.name() != name {
                return Err(conflict(
                    aggregate,
                    Expectation::Status(name.clone()),
                    Observed::Status(status.name().to_owned()),
                ));
            }
        }
    }
    if token_taken(state, action) {
        return Err(CommitError::Constraint(
            "chat token already used".to_owned(),
        ));
    }
    state.actions.insert(action.id(), action.clone());
    Ok(())
}

fn apply_note(
    state: &mut State,
    note: &Note,
    expected: Expected,
    receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    let aggregate = AggregateRef::note(note.id);
    let exists = state.notes.contains_key(&note.id);
    match expected {
        Expected::New if exists => {
            let stored = version(state, &aggregate);
            Err(conflict(
                aggregate,
                Expectation::New,
                Observed::Version(stored),
            ))
        }
        Expected::New => {
            state.notes.insert(note.id, note.clone());
            set_version(state, aggregate.clone(), 1);
            receipt.versions.push((aggregate, 1));
            Ok(())
        }
        Expected::Version(n) if !exists => Err(conflict(
            aggregate,
            Expectation::Version(n),
            Observed::Missing,
        )),
        Expected::Version(n) => {
            let stored = version(state, &aggregate);
            if stored != n {
                return Err(conflict(
                    aggregate,
                    Expectation::Version(n),
                    Observed::Version(stored),
                ));
            }
            state.notes.insert(note.id, note.clone());
            set_version(state, aggregate.clone(), n + 1);
            receipt.versions.push((aggregate, n + 1));
            Ok(())
        }
    }
}
