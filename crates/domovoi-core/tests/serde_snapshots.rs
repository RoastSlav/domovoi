//! JSON wire-format snapshots for the types that cross crate and process boundaries.

use domovoi_core::{
    ActionId, ActionProposal, Advisory, AdvisoryId, AdvisoryKind, Card, CardBadge, CardButton,
    CheckKey, Digest, Evidence, EvidenceRef, EvidenceValue, HostDigest, HostId, HostName, Incident,
    IncidentEvent, IncidentId, IncidentPolicy, ReadTool, Severity, Signal, SignalClass, SignalId,
    SkillName, SourceKind, TargetKey, Trend, TriageReport, Verdict, apply,
};
use jiff::{SignedDuration, Timestamp, civil::date};
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

#[cfg(test)]
fn incident_id() -> IncidentId {
    IncidentId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0003))
}

#[test]
fn triage_report_json_snapshot() {
    let report = TriageReport {
        headline: "plex is crash-looping after the 1.41 image update".to_owned(),
        summary:
            "plex restarted 7 times in 10 minutes, each time after a database migration error."
                .to_owned(),
        cause: "The new image expects a schema the existing library database does not have."
            .to_owned(),
        evidence: vec![
            EvidenceRef {
                label: "restarts".to_owned(),
                value: "7 in 10m".to_owned(),
                source: "docker_inspect plex".to_owned(),
            },
            EvidenceRef {
                label: "last error".to_owned(),
                value: "migration 42 failed".to_owned(),
                source: "docker_logs plex".to_owned(),
            },
        ],
        verdict: Verdict::ActionNeeded,
        next_step: "Pin the previous image tag and restart.".to_owned(),
        proposals: vec![ActionProposal {
            skill: SkillName::parse("restart_container").unwrap(),
            args: [("container".to_owned(), "plex".to_owned())].into(),
            reason: "Restart on the previous tag once it is pinned.".to_owned(),
        }],
        details: "Full reasoning goes here.".to_owned(),
    };
    report.validate().unwrap();
    insta::assert_json_snapshot!(report);
}

#[test]
fn card_json_snapshot() {
    let action = ActionId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0004));
    let card = Card {
        incident: incident_id(),
        host: HostName::parse("nas").unwrap(),
        badge: CardBadge::Verdict(Verdict::ActionNeeded),
        headline: "plex is crash-looping after the 1.41 image update".to_owned(),
        bullets: vec![
            "7 restarts in 10 minutes".to_owned(),
            "migration 42 failed".to_owned(),
        ],
        next_step: Some("Pin the previous image tag and restart.".to_owned()),
        buttons: vec![
            CardButton::Ack,
            CardButton::Mute1h,
            CardButton::Details,
            CardButton::Approve {
                action,
                token: "tok_test".to_owned(),
            },
            CardButton::Deny {
                action,
                token: "tok_test".to_owned(),
            },
            CardButton::OpenInWeb { action },
        ],
        details: Some("Full reasoning goes here.".to_owned()),
        resolved: false,
    };
    card.validate().unwrap();
    insta::assert_json_snapshot!(card);
}

#[test]
fn digest_json_snapshot() {
    let digest = Digest {
        date: date(2026, 10, 31),
        hosts: vec![
            HostDigest {
                host: HostName::parse("nas").unwrap(),
                quiet: false,
                lines: vec!["tank grew 120 GB this week".to_owned()],
                trends: vec![Trend {
                    key: "zfs.tank.used_pct".to_owned(),
                    delta: 1.5,
                    period: SignedDuration::from_hours(7 * 24),
                    projection: Some("full in about 40 days".to_owned()),
                }],
            },
            HostDigest {
                host: HostName::parse("pi").unwrap(),
                quiet: true,
                lines: Vec::new(),
                trends: Vec::new(),
            },
        ],
        open_incidents: vec![(incident_id(), "plex is crash-looping".to_owned())],
        advisories: vec![Advisory {
            id: AdvisoryId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0005)),
            kind: AdvisoryKind::StaleImage,
            host: Some(fixed_host()),
            title: "sonarr image is 120 days old".to_owned(),
            detail: "Pull and recreate the container.".to_owned(),
            created_at: ts(0),
            resolved_at: None,
        }],
    };
    insta::assert_json_snapshot!(digest);
}

#[test]
fn read_tool_schemas_snapshot() {
    let schemas: serde_json::Map<String, serde_json::Value> = ReadTool::ALL
        .iter()
        .map(|tool| (tool.name().to_owned(), tool.json_schema()))
        .collect();
    insta::assert_json_snapshot!(schemas);
}
