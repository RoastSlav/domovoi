//! Grouped incidents track each member fingerprint and stay firing until all have cleared.

use domovoi_core::{
    CheckKey, Fingerprint, HostId, Incident, IncidentEvent, IncidentPolicy, Severity, Signal,
    SignalClass, SourceKind, TargetKey, apply,
};
use jiff::Timestamp;

#[cfg(test)]
fn signal(host: HostId, check: &str, source: SourceKind) -> Signal {
    Signal::firing(
        source,
        host,
        CheckKey::parse(check).unwrap(),
        Some(TargetKey::parse("plex").unwrap()),
        Severity::Warning,
        format!("{check} firing"),
        Timestamp::UNIX_EPOCH,
    )
}

#[test]
fn grouped_incident_fires_until_every_member_clears() {
    let host = HostId::new();
    let kuma = signal(
        host,
        "kuma.down",
        SourceKind::Webhook {
            kind: "uptime-kuma".to_owned(),
        },
    );
    let container = signal(host, "container.unhealthy", SourceKind::Agent);
    let mut incident = Incident::new_pending(&kuma, Timestamp::UNIX_EPOCH);
    let firing = IncidentEvent::Firing {
        severity: Severity::Warning,
        class: SignalClass::Immediate,
        observed_at: Timestamp::UNIX_EPOCH,
        value: None,
    };
    apply(
        &mut incident,
        firing,
        &IncidentPolicy::default(),
        Timestamp::UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(incident.source(), Some(kuma.source()));

    // A single-fingerprint incident has no members; marking passes `firing` through.
    assert!(!incident.mark_member(kuma.fingerprint(), false));
    assert!(incident.is_member(&kuma.fingerprint()));
    assert!(!incident.is_member(&container.fingerprint()));

    incident.relate(container.fingerprint(), true);
    assert!(incident.is_member(&container.fingerprint()));
    let related: Vec<&Fingerprint> = incident.related().collect();
    assert_eq!(related, [&container.fingerprint()]);

    assert!(
        incident.mark_member(kuma.fingerprint(), false),
        "the container still fires"
    );
    assert!(
        !incident.mark_member(container.fingerprint(), false),
        "all clear"
    );
    assert!(incident.mark_member(container.fingerprint(), true));
    // Unknown fingerprints change nothing.
    let stranger = signal(host, "other.check", SourceKind::Probe).fingerprint();
    assert!(incident.mark_member(stranger, false));
    assert!(!incident.is_member(&stranger));

    let json = serde_json::to_string(&incident).unwrap();
    let back: Incident = serde_json::from_str(&json).unwrap();
    assert_eq!(back, incident, "members survive a round trip");
}
