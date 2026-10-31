//! Fixed incidents, reports, time zones and events for the notification tests.

use domovoi_core::{
    CheckKey, Evidence, EvidenceRef, EvidenceValue, HostId, HostName, Incident, IncidentEvent,
    IncidentId, IncidentPolicy, Severity, Signal, SignalClass, SourceKind, TargetKey, TriageReport,
    Verdict, apply,
};
use domovoi_engine::notify::{NotifySettings, QuietHours};
use domovoi_engine::{DomainEvent, EventKind};
use jiff::Timestamp;
use jiff::civil::Time;
use jiff::tz::TimeZone;
use uuid::Uuid;

pub fn host_id() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

pub fn host() -> HostName {
    HostName::parse("nas").unwrap()
}

pub fn ts(text: &str) -> Timestamp {
    text.parse().unwrap()
}

/// Europe/Sofia as a POSIX rule, so tests need no system time zone database.
pub fn sofia() -> TimeZone {
    TimeZone::posix("EET-2EEST,M3.5.0/3,M10.5.0/4").unwrap()
}

pub fn quiet(start: (i8, i8), end: (i8, i8), tz: TimeZone) -> NotifySettings {
    let mut settings = NotifySettings::new(tz);
    settings.quiet_hours = Some(QuietHours {
        start: Time::constant(start.0, start.1, 0, 0),
        end: Time::constant(end.0, end.1, 0, 0),
    });
    settings
}

pub fn signal(check: &str, target: Option<&str>, severity: Severity, at: Timestamp) -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert("restarts", EvidenceValue::Int(7));
    evidence.insert("window", EvidenceValue::Text("10m".to_owned()));
    Signal::firing(
        SourceKind::Agent,
        host_id(),
        CheckKey::parse(check).unwrap(),
        target.map(|t| TargetKey::parse(t).unwrap()),
        severity,
        format!("{} restarted 7 times in 10 minutes", target.unwrap_or("it")),
        at,
    )
    .with_class(SignalClass::Immediate)
    .with_evidence(evidence)
}

/// An open incident from `signal`, with a fixed id.
pub fn open_from(signal: &Signal, n: u128) -> Incident {
    let at = signal.observed_at();
    let mut incident = Incident::new_pending(signal, at).with_id(IncidentId::from_uuid(
        Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_1000 + n),
    ));
    let event = IncidentEvent::Firing {
        severity: signal.severity(),
        class: signal.class(),
        observed_at: at,
        value: None,
    };
    apply(&mut incident, event, &IncidentPolicy::default(), at).unwrap();
    incident
}

pub fn open(severity: Severity, at: Timestamp) -> Incident {
    open_from(
        &signal("container.restart_loop", Some("plex"), severity, at),
        1,
    )
}

pub fn resolve(incident: &mut Incident, at: Timestamp) {
    let event = IncidentEvent::Resolved { observed_at: at };
    apply(incident, event, &IncidentPolicy::default(), at).unwrap();
}

pub fn report(verdict: Verdict) -> TriageReport {
    TriageReport {
        headline: "plex is crash-looping after the 1.41 image update".to_owned(),
        summary: "The container exits during startup.".to_owned(),
        cause: "Database migration 42 fails on the new image.".to_owned(),
        evidence: vec![
            EvidenceRef {
                label: "restarts".to_owned(),
                value: "7 in 10 minutes".to_owned(),
                source: "docker_inspect plex".to_owned(),
            },
            EvidenceRef {
                label: "last error".to_owned(),
                value: "migration 42 failed".to_owned(),
                source: "docker_logs plex".to_owned(),
            },
        ],
        verdict,
        next_step: "Pin the previous image tag and restart.".to_owned(),
        proposals: Vec::new(),
        details: String::new(),
    }
}

pub fn event(kind: EventKind, incident: &Incident) -> DomainEvent {
    DomainEvent::new(kind, incident, true)
}
