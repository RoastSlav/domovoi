//! Tests for which security events a report has not seen.

use jiff::Timestamp;
use serde_json::{Value, json};

use super::{SECURITY_EVENT_KIND, is_security_event, uncovered_events};
use crate::blind::BLIND_TARGET;
use crate::id::{HostId, IncidentId};
use crate::ports::IncidentTimelineEntry;
use crate::severity::Severity;
use crate::signal::{CheckKey, Signal, SourceKind, TargetKey};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(seconds).expect("in range")
}

fn entry(seconds: i64, kind: &str, detail: Value) -> IncidentTimelineEntry {
    IncidentTimelineEntry {
        incident: IncidentId::new(),
        at: at(seconds),
        kind: kind.to_owned(),
        summary: format!("{kind} at {seconds}"),
        detail,
    }
}

fn check() -> CheckKey {
    CheckKey::parse("security.sudo_failure").expect("a key")
}

fn counted(entries: &[IncidentTimelineEntry]) -> Vec<&str> {
    uncovered_events(&check(), entries)
        .iter()
        .map(|e| e.summary.as_str())
        .collect()
}

#[test]
fn without_a_report_there_is_no_verdict_to_qualify() {
    assert!(counted(&[entry(10, SECURITY_EVENT_KIND, Value::Null)]).is_empty());
}

#[test]
fn a_report_covers_only_what_its_run_had_seen() {
    let entries = [
        entry(10, SECURITY_EVENT_KIND, Value::Null),
        entry(30, SECURITY_EVENT_KIND, Value::Null),
        entry(40, "triage.report", json!({"seen_until": at(20)})),
        entry(40, SECURITY_EVENT_KIND, Value::Null),
    ];
    assert_eq!(
        counted(&entries),
        ["security.event at 30", "security.event at 40"]
    );
}

#[test]
fn a_report_without_the_time_covers_up_to_when_it_was_stored() {
    let entries = [
        entry(10, SECURITY_EVENT_KIND, Value::Null),
        entry(20, "triage.report", Value::Null),
        entry(20, SECURITY_EVENT_KIND, Value::Null),
        entry(25, "triage.failed", Value::Null),
        entry(30, SECURITY_EVENT_KIND, Value::Null),
    ];
    assert_eq!(counted(&entries), ["security.event at 30"]);
}

#[test]
fn repeated_states_and_blindness_are_not_events() {
    let signal = |check: &str, target: &str| {
        Signal::firing(
            SourceKind::Agent,
            HostId::new(),
            CheckKey::parse(check).expect("a key"),
            Some(TargetKey::parse(target).expect("a target")),
            Severity::Warning,
            "firing",
            at(0),
        )
    };
    assert!(is_security_event(&signal("security.ssh_login", "bob")));
    assert!(!is_security_event(&signal(
        "security.new_port",
        "0.0.0.0:22/tcp"
    )));
    assert!(!is_security_event(&signal(
        "security.sudo_failure",
        BLIND_TARGET
    )));
}
