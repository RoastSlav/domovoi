//! Whether the system itself recorded a Domovoi action that explains a security event.
//!
//! A model's word never blames Domovoi for a security event. Only a record does: an
//! execution the hub dispatched to the same host, close in time to every event of the
//! incident, whose target or arguments the event names. This runs in code, over stored
//! records; the model is not asked and its text is not read. The lookup is the hub's record
//! of what it handed to an agent (operation, contract, arguments, dispatch and settle time),
//! which is also what the agent's action journal answers to.

use domovoi_core::triage::is_security_check;
use domovoi_core::{
    CheckKey, EvidenceValue, ExecutionRecord, HostId, Incident, Signal, SignalStatus,
};
use jiff::{SignedDuration, Timestamp};

use super::Shared;

/// How long before its dispatch an action may be blamed for an event, for clock skew.
const LEAD: SignedDuration = SignedDuration::from_secs(60);
/// How long after an action settled an event may still be blamed on it.
const TAIL: SignedDuration = SignedDuration::from_mins(5);
/// How long after dispatch an action that has not settled may be blamed.
const UNSETTLED: SignedDuration = SignedDuration::from_mins(10);
/// How far before the oldest event the records are read. A longer run is not "close".
const LOOKBACK: SignedDuration = SignedDuration::from_mins(30);
/// Most events of one incident that are read.
const MAX_EVENTS: u16 = 20;
/// Values too common to say anything about an event.
const COMMON: &[&str] = &[
    "true", "false", "yes", "none", "null", "all", "any", "auto", "default", "service",
];

/// What one security event says about itself.
struct Event {
    at: Timestamp,
    target: Option<String>,
    /// Lowercase summary, target and every text evidence value.
    text: String,
}

impl Event {
    fn of(signal: &Signal) -> Self {
        let mut text = signal.summary().to_lowercase();
        if let Some(target) = signal.target() {
            text.push(' ');
            text.push_str(&target.as_str().to_lowercase());
        }
        for (_, value) in signal.evidence().iter() {
            if let EvidenceValue::Text(value) = value {
                text.push(' ');
                text.push_str(&value.to_lowercase());
            }
        }
        Self {
            at: signal.observed_at(),
            target: signal.target().map(|t| t.as_str().to_owned()),
            text,
        }
    }
}

/// Whether `text` holds `needle` as a whole word: not inside a longer word, name or path
/// part. `.` and `/` end a word, so `nginx` is found in `nginx.service`.
fn has_word(text: &str, needle: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    text.match_indices(needle).any(|(at, _)| {
        text[..at].chars().next_back().is_none_or(|c| !word(c))
            && text[at + needle.len()..]
                .chars()
                .next()
                .is_none_or(|c| !word(c))
    })
}

/// The words of a record that an event could name: its argument values and its target, with a
/// `.service` suffix dropped. Words of the check's own name and common words are left out, so
/// `sudo` in a package name does not explain every sudo failure.
fn needles(record: &ExecutionRecord, check: &CheckKey) -> Vec<String> {
    let own: Vec<&str> = check
        .as_str()
        .split(|c: char| !c.is_alphanumeric())
        .collect();
    let mut raw: Vec<String> = record.args.values().cloned().collect();
    if let Some(target) = &record.target {
        raw.push(target.as_str().to_owned());
        if let Some((_, name)) = target.as_str().split_once(':') {
            raw.push(name.to_owned());
        }
    }
    raw.into_iter()
        .map(|value| {
            let value = value.trim().to_lowercase();
            value
                .strip_suffix(".service")
                .map_or(value.clone(), str::to_owned)
        })
        .filter(|value| {
            value.chars().count() >= 3
                && !COMMON.contains(&value.as_str())
                && !own.contains(&value.as_str())
                && !value.chars().all(|c| c.is_ascii_digit())
        })
        .collect()
}

/// Whether `record` is a Domovoi action close in time to `event` whose target or arguments
/// the event names.
fn explains(record: &ExecutionRecord, event: &Event, check: &CheckKey) -> bool {
    let Some((_, dispatched)) = record.dispatched else {
        return false;
    };
    let end = record
        .settled_at
        .map_or(dispatched + UNSETTLED, |settled| settled.max(dispatched));
    if event.at < dispatched - LEAD || event.at > end + TAIL {
        return false;
    }
    if let (Some(target), Some(named)) = (&record.target, &event.target)
        && target.as_str() == named
    {
        return true;
    }
    needles(record, check)
        .iter()
        .any(|needle| has_word(&event.text, needle))
}

/// The records among `records` that together explain every one of `events`, or `None` when
/// any event has no record that does. One unexplained event keeps the whole incident
/// unexplained: a restart does not excuse a later, unrelated failure.
fn explaining<'a>(
    check: &CheckKey,
    events: &[Event],
    records: &'a [ExecutionRecord],
) -> Option<Vec<&'a ExecutionRecord>> {
    if events.is_empty() {
        return None;
    }
    let mut found: Vec<&ExecutionRecord> = Vec::new();
    for event in events {
        let record = records
            .iter()
            .find(|record| explains(record, event, check))?;
        if !found.iter().any(|seen| seen.execution == record.execution) {
            found.push(record);
        }
    }
    Some(found)
}

/// The one-line description of the records, for the report: operation id, time and what ran.
fn describe(records: &[&ExecutionRecord]) -> String {
    let lines: Vec<String> = records
        .iter()
        .take(3)
        .map(|record| {
            let args: Vec<String> = record
                .args
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            let when = record
                .dispatched
                .map_or_else(String::new, |(_, at)| format!(", dispatched {at}"));
            let settled = record.settled_at.map_or_else(
                || ", not settled".to_owned(),
                |at| format!(", settled {at}"),
            );
            format!(
                "operation {}: {} {}{when}{settled}",
                record.operation,
                record.contract,
                args.join(" ")
            )
        })
        .collect();
    lines.join("; ")
}

/// The recorded actions among `records` that explain every firing event among `signals` of an
/// incident of `check` on `host`, described in one line, or `None`.
fn match_records(
    check: &CheckKey,
    host: HostId,
    signals: &[Signal],
    records: &[ExecutionRecord],
) -> Option<String> {
    let events: Vec<Event> = signals
        .iter()
        .filter(|signal| signal.status() == SignalStatus::Firing && signal.host() == host)
        .map(Event::of)
        .collect();
    let records: Vec<ExecutionRecord> = records
        .iter()
        .filter(|record| record.host == host)
        .cloned()
        .collect();
    explaining(check, &events, &records).map(|found| describe(&found))
}

/// The recorded Domovoi actions that explain `incident`, described in one line, or `None`
/// when there is none: not a security incident, no firing event, a store that cannot be read,
/// or an event no record on the same host, near it in time, names. Reads fail closed.
pub(crate) async fn recorded_action(shared: &Shared, incident: &Incident) -> Option<String> {
    if !is_security_check(incident.check()) {
        return None;
    }
    let deps = &shared.deps;
    let signals = deps
        .signals
        .list_for_incident(incident.id(), MAX_EVENTS)
        .await
        .ok()?;
    let firing = signals
        .iter()
        .filter(|signal| signal.status() == SignalStatus::Firing)
        .map(Signal::observed_at);
    let oldest = firing.clone().min()?;
    let newest = firing.max()?;
    let records = deps
        .experience
        .operations()
        .dispatched_between(incident.host(), oldest - LOOKBACK, newest + LEAD)
        .await
        .ok()?;
    let line = match_records(incident.check(), incident.host(), &signals, &records)?;
    Some(deps.redactor.redact(&line))
}

#[cfg(test)]
#[path = "attribution_tests.rs"]
mod tests;
