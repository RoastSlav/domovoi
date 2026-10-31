//! JSON wire-format snapshots for the types that cross crate and process boundaries.

use domovoi_core::{
    CheckKey, Evidence, EvidenceValue, HostId, Incident, IncidentEvent, IncidentId, IncidentPolicy,
    Severity, Signal, SignalClass, SignalId, SourceKind, TargetKey, apply,
};
use jiff::Timestamp;
use uuid::Uuid;

fn fixed_host() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

#[cfg(test)]
fn ts(secs: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + secs).unwrap()
}

#[cfg(test)]
fn sample_signal() -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert("container", EvidenceValue::Text("plex".to_owned()));
    evidence.insert("restarts", EvidenceValue::Int(7));
    evidence.insert("value", EvidenceValue::Float(7.5));
    evidence.insert("oom_killed", EvidenceValue::Bool(true));
    Signal::firing(
        SourceKind::Webhook {
            kind: "uptime-kuma".to_owned(),
        },
        fixed_host(),
        CheckKey::parse("container.restart_loop").unwrap(),
        Some(TargetKey::parse("plex").unwrap()),
        Severity::Warning,
        "plex restarted 7 times in 10 minutes",
        ts(0),
    )
    .with_class(SignalClass::Normal)
    .with_evidence(evidence)
    .with_id(SignalId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0002,
    )))
}

#[test]
fn signal_json_snapshot() {
    insta::assert_json_snapshot!(sample_signal());
}

#[test]
fn signal_round_trips_through_json() {
    let signal = sample_signal();
    let json = serde_json::to_string(&signal).unwrap();
    let back: Signal = serde_json::from_str(&json).unwrap();
    assert_eq!(back, signal);
}

#[test]
fn incident_json_snapshot() {
    let signal = sample_signal();
    let policy = IncidentPolicy::default();
    let mut incident = Incident::new_pending(&signal, ts(0)).with_id(IncidentId::from_uuid(
        Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0003),
    ));
    for offset in [0, 60] {
        let event = IncidentEvent::Firing {
            severity: signal.severity(),
            class: signal.class(),
            observed_at: ts(offset),
            value: signal.evidence().tracked_value(),
        };
        apply(&mut incident, event, &policy, ts(offset)).unwrap();
    }
    insta::assert_json_snapshot!(incident);
}
