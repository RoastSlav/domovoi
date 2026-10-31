//! The pure decision function, step by step with fixed times.

use domovoi_core::{
    AckRule, AckRuleId, CheckKey, EvidencePredicate, EvidenceValue, HostId, IncidentStatus,
    MaintenanceWindow, Severity, UserId,
};
use domovoi_engine::manager::{Command, CommandError, Write};
use domovoi_engine::{EnginePolicy, EventKind};
use domovoi_testkit::build::at;
use jiff::SignedDuration;

use crate::harness::{Sig, Sim, incident_ids, kinds, kuma};

use EventKind::{
    IncidentEscalated, IncidentFolded, IncidentOpened, IncidentReopened, IncidentResolved,
    TriageRequested,
};

fn folded(kinds: &[EventKind]) -> bool {
    kinds.iter().all(|k| matches!(k, IncidentFolded { .. })) && !kinds.is_empty()
}

#[test]
fn two_firings_confirm_and_open_once() {
    let mut sim = Sim::new(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/"));
    let first = sim.ingest(disk.firing(0), 0);
    assert!(first.events.is_empty(), "pending, not yet confirmed");
    assert_eq!(sim.only_active().status(), IncidentStatus::Pending);
    assert!(matches!(first.writes[0], Write::InsertIncident(_)));

    let second = sim.ingest(disk.firing(60), 60);
    assert_eq!(kinds(&second), [IncidentOpened, TriageRequested]);
    assert_eq!(sim.only_active().status(), IncidentStatus::Open);
    let id = sim.only_active().id();

    let third = sim.ingest(disk.firing(120), 120);
    assert!(folded(&kinds(&third)), "{:?}", kinds(&third));
    assert_eq!(incident_ids(&third), [id]);
}

#[test]
fn immediate_signal_opens_on_first_firing() {
    let mut sim = Sim::new(EnginePolicy::default());
    let power = Sig::new(HostId::new(), "power.supply", None).immediate();
    let decision = sim.ingest(power.firing(0), 0);
    assert_eq!(kinds(&decision), [IncidentOpened, TriageRequested]);
    assert!(decision.events.iter().all(|e| e.notify));
}

#[test]
fn repeat_firing_folds_and_does_not_open_again() {
    let mut sim = Sim::new(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    for (n, secs) in [60, 120, 180].into_iter().enumerate() {
        let signal = disk.firing(secs);
        let decision = sim.ingest(signal.clone(), secs);
        let expected = u32::try_from(n + 2).unwrap();
        assert_eq!(
            kinds(&decision),
            [IncidentFolded {
                signal: signal.id(),
                occurrences: expected,
            }]
        );
    }
    assert_eq!(sim.state.active.len(), 1);
    assert_eq!(sim.only_active().occurrences(), 4);
}

#[test]
fn resolved_then_firing_within_window_reopens_same_incident() {
    let mut sim = Sim::new(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    let id = sim.only_active().id();

    let resolved = sim.ingest(disk.resolved(100), 100);
    assert_eq!(kinds(&resolved), [IncidentResolved]);
    assert!(sim.state.active.is_empty());

    let reopened = sim.ingest(disk.firing(3600), 3600);
    assert_eq!(kinds(&reopened), [IncidentReopened, TriageRequested]);
    assert_eq!(sim.only_active().id(), id, "the same incident");

    // Resolved again, then quiet past the 6-hour window: a new incident.
    sim.ingest(disk.resolved(4000), 4000);
    let later = 4000 + 6 * 3600 + 1;
    let fresh = sim.ingest(disk.firing(later), later);
    assert_eq!(kinds(&fresh), [IncidentOpened, TriageRequested]);
    assert_ne!(sim.only_active().id(), id);
}

#[test]
fn uptime_kuma_down_and_container_unhealthy_group_into_one_incident() {
    let mut sim = Sim::new(EnginePolicy::default());
    let host = HostId::new();
    let web = Sig::new(host, "uptime.down", Some("https://plex.example:32400"))
        .source(kuma())
        .immediate();
    let container = Sig::new(host, "container.unhealthy", Some("media_plex_1"));

    sim.ingest(web.firing(0), 0);
    let id = sim.only_active().id();
    let joined = sim.ingest(container.firing(60), 60);
    assert_eq!(sim.state.active.len(), 1, "one incident, not two");
    assert_eq!(incident_ids(&joined), [id]);
    assert!(folded(&kinds(&joined)));
    assert!(joined.writes.iter().any(|w| matches!(
        w,
        Write::Timeline(entry) if entry.kind == "incident.grouped"
    )));
    let incident = sim.only_active();
    assert!(incident.is_member(&container.firing(0).fingerprint()));

    // Further container signals keep folding into the same incident.
    let again = sim.ingest(container.firing(120), 120);
    assert_eq!(incident_ids(&again), [id]);

    // The web check recovers but the container is still unhealthy: still open.
    let half = sim.ingest(web.resolved(180), 180);
    assert!(half.events.is_empty(), "{:?}", kinds(&half));
    assert_eq!(sim.only_active().status(), IncidentStatus::Open);
    // Both recovered: resolved.
    let done = sim.ingest(container.resolved(240), 240);
    assert_eq!(kinds(&done), [IncidentResolved]);
    assert!(sim.state.active.is_empty());
}

#[test]
fn maintenance_window_suppresses_notify_but_keeps_state() {
    let mut sim = Sim::new(EnginePolicy::default());
    let host = HostId::new();
    sim.state.windows.push(MaintenanceWindow {
        host,
        starts_at: at(0),
        ends_at: at(3600),
        note: "upgrading".to_owned(),
    });
    let disk = Sig::new(host, "disk.free", Some("/")).immediate();
    let inside = sim.ingest(disk.firing(10), 10);
    assert_eq!(kinds(&inside), [IncidentOpened, TriageRequested]);
    assert!(inside.events.iter().all(|e| !e.notify), "suppressed");
    assert_eq!(
        sim.only_active().status(),
        IncidentStatus::Open,
        "state still changes"
    );
    assert!(
        inside
            .writes
            .iter()
            .any(|w| matches!(w, Write::InsertIncident(_))),
        "and is stored"
    );

    // Another host is not in maintenance.
    let other = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    assert!(
        sim.ingest(other.firing(20), 20)
            .events
            .iter()
            .all(|e| e.notify)
    );
    // After the window, changes notify again.
    let after = sim.ingest(disk.resolved(3600), 3600);
    assert!(after.events.iter().all(|e| e.notify));
}

#[test]
fn ack_rule_with_evidence_predicate_suppresses_matching_signals_only() {
    let mut sim = Sim::new(EnginePolicy::default());
    let host = HostId::new();
    sim.state.acks.push(AckRule {
        id: AckRuleId::new(),
        host: Some(host),
        check: CheckKey::parse("disk.free").unwrap(),
        target_pattern: None,
        predicates: vec![EvidencePredicate::Equals {
            key: "mount".to_owned(),
            value: EvidenceValue::Text("/backup".to_owned()),
        }],
        note: "backup disk is meant to be full".to_owned(),
        expires_at: None,
    });
    let backup = Sig::new(host, "disk.free", Some("/backup"))
        .immediate()
        .evidence("mount", EvidenceValue::Text("/backup".to_owned()));
    let root = Sig::new(host, "disk.free", Some("/"))
        .immediate()
        .evidence("mount", EvidenceValue::Text("/".to_owned()));

    let quiet = sim.ingest(backup.firing(0), 0);
    assert_eq!(kinds(&quiet), [IncidentOpened, TriageRequested]);
    assert!(quiet.events.iter().all(|e| !e.notify));
    let loud = sim.ingest(root.firing(10), 10);
    assert!(
        loud.events.iter().all(|e| e.notify),
        "the rule matches /backup only"
    );
    assert_eq!(sim.state.active.len(), 2, "both incidents exist");
}

#[test]
fn ttl_tick_resolves_quiet_incident() {
    let mut policy = EnginePolicy::default();
    policy.incident.ttl = Some(SignedDuration::from_mins(10));
    let mut sim = Sim::new(policy);
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    assert!(
        sim.run(Command::Tick, 9 * 60).events.is_empty(),
        "not quiet long enough"
    );
    let tick = sim.run(Command::Tick, 10 * 60);
    assert_eq!(kinds(&tick), [IncidentResolved]);
    assert!(sim.state.active.is_empty());
    assert!(
        sim.run(Command::Tick, 20 * 60).writes.is_empty(),
        "nothing left to tick"
    );
}

#[test]
fn pending_resolved_before_confirmation_is_deleted_and_counted() {
    let mut sim = Sim::new(EnginePolicy::default());
    let net = Sig::new(HostId::new(), "net.link", Some("eth0"));
    sim.ingest(net.firing(0), 0);
    let id = sim.only_active().id();
    let dropped = sim.ingest(net.resolved(30), 30);
    assert!(dropped.events.is_empty(), "nobody hears about a flap");
    assert_eq!(dropped.flaps, 1);
    assert!(dropped.writes.contains(&Write::DiscardPending(id)));
    assert!(
        dropped
            .writes
            .iter()
            .any(|w| matches!(w, Write::InsertSignal(_, None))),
        "the signal is kept, unlinked"
    );
    assert!(sim.state.active.is_empty());
}

#[test]
fn value_evidence_is_recorded_in_history() {
    let mut sim = Sim::new(EnginePolicy::default());
    let temp = Sig::new(HostId::new(), "cpu.temperature", None).immediate();
    for (secs, value) in [(0, 71.5), (60, 73.0), (120, 74.25)] {
        let signal = temp
            .clone()
            .evidence("value", EvidenceValue::Float(value))
            .firing(secs);
        sim.ingest(signal, secs);
    }
    let history: Vec<f64> = sim
        .only_active()
        .history()
        .iter()
        .map(|(_, v)| *v)
        .collect();
    assert_eq!(history, [71.5, 73.0, 74.25]);
}

#[test]
fn escalation_requests_triage() {
    let mut sim = Sim::new(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    let worse = sim.ingest(disk.clone().severity(Severity::Critical).firing(60), 60);
    assert_eq!(
        kinds(&worse)[..2],
        [
            IncidentEscalated {
                from: Severity::Warning,
                to: Severity::Critical
            },
            TriageRequested
        ]
    );
    assert_eq!(sim.only_active().severity(), Severity::Critical);
}

#[test]
fn illegal_ack_is_an_error_without_writes() {
    let mut sim = Sim::new(EnginePolicy::default());
    let user = UserId::new();
    let unknown = domovoi_core::IncidentId::new();
    assert_eq!(
        sim.try_run(
            Command::Ack {
                incident: unknown,
                by: user
            },
            0
        ),
        Err(CommandError::UnknownIncident(unknown))
    );
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    let id = sim.only_active().id();
    let acked = sim.run(
        Command::Ack {
            incident: id,
            by: user,
        },
        10,
    );
    assert_eq!(kinds(&acked), [EventKind::IncidentAcked { by: user }]);
    let before = sim.state.clone();
    assert!(matches!(
        sim.try_run(
            Command::Ack {
                incident: id,
                by: user
            },
            20
        ),
        Err(CommandError::NotAllowed(_))
    ));
    assert_eq!(sim.state, before, "nothing changed");

    let until = at(600);
    let muted = sim.run(
        Command::Mute {
            incident: id,
            until,
            by: user,
        },
        30,
    );
    assert_eq!(kinds(&muted), [EventKind::IncidentMuted { until }]);
    let unmuted = sim.run(Command::Unmute { incident: id }, 40);
    assert_eq!(kinds(&unmuted), [EventKind::IncidentUnmuted]);
}
