//! One test per arrow of the incident state diagram, plus illegal-transition and invariant checks.

use domovoi_core::{
    CheckKey, HostId, Incident, IncidentChange, IncidentEvent, IncidentPolicy, IncidentStatus,
    Severity, Signal, SignalClass, SourceKind, TransitionError, UserId, apply,
};
use jiff::{SignedDuration, Timestamp};
use proptest::prelude::*;

#[cfg(test)]
fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + secs).unwrap()
}

fn hours(h: i64) -> SignedDuration {
    SignedDuration::from_hours(h)
}

#[cfg(test)]
fn pending(now: Timestamp) -> Incident {
    let signal = Signal::firing(
        SourceKind::BuiltIn,
        HostId::new(),
        CheckKey::parse("container.restart_loop").unwrap(),
        None,
        Severity::Warning,
        "plex is restarting",
        now,
    );
    Incident::new_pending(&signal, now)
}

fn firing(severity: Severity, observed_at: Timestamp) -> IncidentEvent {
    IncidentEvent::Firing {
        severity,
        class: SignalClass::Normal,
        observed_at,
        value: None,
    }
}

fn resolved(observed_at: Timestamp) -> IncidentEvent {
    IncidentEvent::Resolved { observed_at }
}

fn ack() -> IncidentEvent {
    IncidentEvent::Ack { by: UserId::new() }
}

#[cfg(test)]
fn run(
    inc: &mut Incident,
    event: IncidentEvent,
    policy: &IncidentPolicy,
    now: Timestamp,
) -> Vec<IncidentChange> {
    apply(inc, event, policy, now).unwrap()
}

/// An open incident with the given policy, confirmed at `at(60)`.
fn open_incident(policy: &IncidentPolicy) -> Incident {
    let mut inc = pending(at(0));
    run(&mut inc, firing(Severity::Warning, at(0)), policy, at(0));
    run(&mut inc, firing(Severity::Warning, at(60)), policy, at(60));
    assert_eq!(inc.status(), IncidentStatus::Open);
    inc
}

#[test]
fn pending_confirms_after_confirm_runs_firings() {
    let policy = IncidentPolicy::default();
    let mut inc = pending(at(0));
    let first = run(&mut inc, firing(Severity::Warning, at(0)), &policy, at(0));
    assert_eq!(inc.status(), IncidentStatus::Pending);
    assert!(!first.contains(&IncidentChange::Confirmed));
    let second = run(&mut inc, firing(Severity::Warning, at(60)), &policy, at(60));
    assert_eq!(inc.status(), IncidentStatus::Open);
    assert_eq!(inc.confirmed_at(), Some(at(60)));
    assert!(second.contains(&IncidentChange::Confirmed));
    assert!(second.contains(&IncidentChange::StatusChanged {
        from: IncidentStatus::Pending,
        to: IncidentStatus::Open
    }));
}

#[test]
fn immediate_signal_opens_without_confirmation() {
    let policy = IncidentPolicy::default();
    let mut inc = pending(at(0));
    let event = IncidentEvent::Firing {
        severity: Severity::Critical,
        class: SignalClass::Immediate,
        observed_at: at(0),
        value: None,
    };
    let changes = run(&mut inc, event, &policy, at(0));
    assert_eq!(inc.status(), IncidentStatus::Open);
    assert!(changes.contains(&IncidentChange::Confirmed));
    assert_eq!(inc.occurrences(), 1);
}

#[test]
fn pending_resolved_before_confirmation_is_dropped() {
    let policy = IncidentPolicy::default();
    let mut inc = pending(at(0));
    run(&mut inc, firing(Severity::Warning, at(0)), &policy, at(0));
    let changes = run(&mut inc, resolved(at(30)), &policy, at(30));
    assert_eq!(changes, vec![IncidentChange::Dropped]);
}

#[test]
fn open_firing_folds_and_raises_severity() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    let before = inc.occurrences();
    let changes = run(
        &mut inc,
        firing(Severity::Critical, at(120)),
        &policy,
        at(120),
    );
    assert_eq!(inc.severity(), Severity::Critical);
    assert_eq!(inc.occurrences(), before + 1);
    assert!(changes.contains(&IncidentChange::Folded {
        occurrences: before + 1
    }));
    assert!(changes.contains(&IncidentChange::SeverityChanged {
        from: Severity::Warning,
        to: Severity::Critical
    }));
    assert_eq!(inc.last_firing_at(), at(120));
}

#[test]
fn severity_never_falls_while_firing() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(
        &mut inc,
        firing(Severity::Critical, at(120)),
        &policy,
        at(120),
    );
    run(&mut inc, firing(Severity::Info, at(180)), &policy, at(180));
    assert_eq!(inc.severity(), Severity::Critical);
}

#[test]
fn firing_value_is_recorded_in_history() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    let event = IncidentEvent::Firing {
        severity: Severity::Warning,
        class: SignalClass::Normal,
        observed_at: at(120),
        value: Some(7.0),
    };
    let changes = run(&mut inc, event, &policy, at(120));
    assert!(changes.contains(&IncidentChange::ValueRecorded { value: 7.0 }));
    assert_eq!(inc.history().latest(), Some((at(120), 7.0)));
}

#[test]
fn open_acks_to_acked() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(&mut inc, ack(), &policy, at(100));
    assert_eq!(inc.status(), IncidentStatus::Acked);
}

#[test]
fn muted_acks_to_acked() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(
        &mut inc,
        IncidentEvent::Mute { until: at(3600) },
        &policy,
        at(100),
    );
    run(&mut inc, ack(), &policy, at(110));
    assert_eq!(inc.status(), IncidentStatus::Acked);
}

#[test]
fn acked_mutes_and_unmutes_to_open() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(&mut inc, ack(), &policy, at(100));
    run(
        &mut inc,
        IncidentEvent::Mute { until: at(3600) },
        &policy,
        at(110),
    );
    assert_eq!(inc.status(), IncidentStatus::Muted { until: at(3600) });
    run(&mut inc, IncidentEvent::Unmute, &policy, at(120));
    assert_eq!(inc.status(), IncidentStatus::Open);
}

#[test]
fn firing_folds_into_acked_and_muted_incidents() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(&mut inc, ack(), &policy, at(100));
    let changes = run(
        &mut inc,
        firing(Severity::Warning, at(130)),
        &policy,
        at(130),
    );
    assert_eq!(inc.status(), IncidentStatus::Acked);
    assert!(matches!(
        changes.last(),
        Some(IncidentChange::Folded { .. })
    ));
    run(
        &mut inc,
        IncidentEvent::Mute { until: at(3600) },
        &policy,
        at(140),
    );
    let changes = run(
        &mut inc,
        firing(Severity::Warning, at(150)),
        &policy,
        at(150),
    );
    assert!(matches!(
        changes.last(),
        Some(IncidentChange::Folded { .. })
    ));
    assert_eq!(inc.status(), IncidentStatus::Muted { until: at(3600) });
}

#[test]
fn muted_returns_to_open_after_until() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(
        &mut inc,
        IncidentEvent::Mute { until: at(600) },
        &policy,
        at(100),
    );
    assert!(run(&mut inc, IncidentEvent::Tick, &policy, at(599)).is_empty());
    assert_eq!(inc.status(), IncidentStatus::Muted { until: at(600) });
    run(&mut inc, IncidentEvent::Tick, &policy, at(600));
    assert_eq!(inc.status(), IncidentStatus::Open);
}

#[test]
fn open_acked_and_muted_resolve_on_resolved_signal() {
    let policy = IncidentPolicy::default();
    for prepare in 0..3 {
        let mut inc = open_incident(&policy);
        match prepare {
            1 => {
                run(&mut inc, ack(), &policy, at(100));
            }
            2 => {
                run(
                    &mut inc,
                    IncidentEvent::Mute { until: at(3600) },
                    &policy,
                    at(100),
                );
            }
            _ => {}
        }
        run(&mut inc, resolved(at(200)), &policy, at(200));
        assert_eq!(inc.status(), IncidentStatus::Resolved);
        assert_eq!(inc.resolved_at(), Some(at(200)));
    }
}

#[test]
fn ttl_resolves_quiet_incident() {
    let policy = IncidentPolicy {
        ttl: Some(hours(1)),
        ..IncidentPolicy::default()
    };
    let mut inc = open_incident(&policy);
    run(&mut inc, IncidentEvent::Tick, &policy, at(60 + 3599));
    assert_eq!(inc.status(), IncidentStatus::Open);
    run(&mut inc, IncidentEvent::Tick, &policy, at(60 + 3600));
    assert_eq!(inc.status(), IncidentStatus::Resolved);
    assert_eq!(inc.resolved_at(), Some(at(60 + 3600)));
}

#[test]
fn ttl_resolves_acked_and_muted_incidents_too() {
    let policy = IncidentPolicy {
        ttl: Some(hours(1)),
        ..IncidentPolicy::default()
    };
    let mut acked = open_incident(&policy);
    run(&mut acked, ack(), &policy, at(100));
    run(&mut acked, IncidentEvent::Tick, &policy, at(10_000));
    assert_eq!(acked.status(), IncidentStatus::Resolved);

    let mut muted = open_incident(&policy);
    run(
        &mut muted,
        IncidentEvent::Mute { until: at(100_000) },
        &policy,
        at(100),
    );
    run(&mut muted, IncidentEvent::Tick, &policy, at(10_000));
    assert_eq!(muted.status(), IncidentStatus::Resolved);
}

#[test]
fn tick_does_nothing_without_ttl_and_on_pending_or_resolved() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    assert!(run(&mut inc, IncidentEvent::Tick, &policy, at(10_000_000)).is_empty());
    assert_eq!(inc.status(), IncidentStatus::Open);
    let mut p = pending(at(0));
    assert!(run(&mut p, IncidentEvent::Tick, &policy, at(10)).is_empty());
    run(&mut inc, resolved(at(200)), &policy, at(200));
    assert!(run(&mut inc, IncidentEvent::Tick, &policy, at(300)).is_empty());
}

#[test]
fn resolved_reopens_within_window() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(&mut inc, resolved(at(200)), &policy, at(200));
    let before = inc.occurrences();
    let later = at(200 + 5 * 3600);
    let changes = run(&mut inc, firing(Severity::Info, later), &policy, later);
    assert_eq!(inc.status(), IncidentStatus::Open);
    assert_eq!(inc.resolved_at(), None);
    assert_eq!(inc.occurrences(), before + 1);
    assert!(inc.confirmed_at().is_some());
    assert!(changes.contains(&IncidentChange::Reopened));
}

#[test]
fn resolved_after_window_reports_elapsed() {
    let policy = IncidentPolicy::default();
    let mut inc = open_incident(&policy);
    run(&mut inc, resolved(at(200)), &policy, at(200));
    let snapshot = inc.clone();
    let late = at(200 + 7 * 3600);
    let err = apply(&mut inc, firing(Severity::Warning, late), &policy, late).unwrap_err();
    assert_eq!(err, TransitionError::ReopenWindowElapsed);
    assert_eq!(inc, snapshot);
}

#[test]
fn illegal_transitions_leave_incident_unchanged() {
    let policy = IncidentPolicy::default();
    let acked = {
        let mut inc = open_incident(&policy);
        run(&mut inc, ack(), &policy, at(100));
        inc
    };
    let muted = {
        let mut inc = open_incident(&policy);
        run(
            &mut inc,
            IncidentEvent::Mute { until: at(9000) },
            &policy,
            at(100),
        );
        inc
    };
    let done = {
        let mut inc = open_incident(&policy);
        run(&mut inc, resolved(at(200)), &policy, at(200));
        inc
    };
    let cases: Vec<(&str, Incident, IncidentEvent)> = vec![
        ("pending ack", pending(at(0)), ack()),
        (
            "pending mute",
            pending(at(0)),
            IncidentEvent::Mute { until: at(9000) },
        ),
        ("pending unmute", pending(at(0)), IncidentEvent::Unmute),
        ("open unmute", open_incident(&policy), IncidentEvent::Unmute),
        ("acked ack", acked.clone(), ack()),
        ("acked unmute", acked, IncidentEvent::Unmute),
        ("muted mute", muted, IncidentEvent::Mute { until: at(9999) }),
        ("resolved resolved", done.clone(), resolved(at(500))),
        ("resolved ack", done.clone(), ack()),
        (
            "resolved mute",
            done.clone(),
            IncidentEvent::Mute { until: at(9000) },
        ),
        ("resolved unmute", done, IncidentEvent::Unmute),
    ];
    for (name, base, event) in cases {
        let mut inc = base.clone();
        let err = apply(&mut inc, event, &policy, at(500)).unwrap_err();
        assert!(
            matches!(err, TransitionError::Illegal { .. }),
            "{name}: {err:?}"
        );
        assert_eq!(inc, base, "{name} changed the incident");
    }
}

fn arb_severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Info),
        Just(Severity::Warning),
        Just(Severity::Critical)
    ]
}

#[derive(Debug, Clone)]
enum Step {
    Firing(Severity, bool, Option<i32>),
    Resolved,
    Ack,
    Mute(u32),
    Unmute,
    Tick,
}

fn arb_step() -> impl Strategy<Value = (Step, u32)> {
    let step = prop_oneof![
        (
            arb_severity(),
            any::<bool>(),
            proptest::option::of(-1000..1000i32)
        )
            .prop_map(|(s, imm, v)| Step::Firing(s, imm, v)),
        Just(Step::Resolved),
        Just(Step::Ack),
        (1..20_000u32).prop_map(Step::Mute),
        Just(Step::Unmute),
        Just(Step::Tick),
    ];
    (step, 0..8_000u32)
}

proptest! {
    #[test]
    fn transitions_never_panic_and_keep_invariants(
        confirm_runs in 1u8..4,
        ttl in proptest::option::of(1i64..4),
        steps in proptest::collection::vec(arb_step(), 0..60),
    ) {
        let policy = IncidentPolicy {
            confirm_runs,
            reopen_window: hours(2),
            ttl: ttl.map(hours),
        };
        let mut inc = pending(at(0));
        let mut clock = 0i64;
        let mut max_severity: Option<Severity> = None;
        for (step, dt) in steps {
            clock += i64::from(dt);
            let now = at(clock);
            let event = match step {
                Step::Firing(severity, imm, value) => IncidentEvent::Firing {
                    severity,
                    class: if imm { SignalClass::Immediate } else { SignalClass::Normal },
                    observed_at: now,
                    value: value.map(f64::from),
                },
                Step::Resolved => resolved(now),
                Step::Ack => ack(),
                Step::Mute(secs) => IncidentEvent::Mute { until: at(clock + i64::from(secs)) },
                Step::Unmute => IncidentEvent::Unmute,
                Step::Tick => IncidentEvent::Tick,
            };
            let was_resolved = inc.status() == IncidentStatus::Resolved;
            let firing_severity = match &event {
                IncidentEvent::Firing { severity, .. } => Some(*severity),
                _ => None,
            };
            let before = inc.clone();
            match apply(&mut inc, event, &policy, now) {
                Ok(_) => {
                    if let Some(sev) = firing_severity {
                        max_severity = Some(match max_severity {
                            Some(m) if !was_resolved => m.max(sev),
                            _ => sev,
                        });
                    }
                    if inc.status() == IncidentStatus::Resolved {
                        max_severity = None;
                    }
                }
                Err(_) => prop_assert_eq!(&inc, &before),
            }
            prop_assert_eq!(
                inc.resolved_at().is_some(),
                inc.status() == IncidentStatus::Resolved
            );
            if inc.status() != IncidentStatus::Pending {
                prop_assert!(inc.confirmed_at().is_some());
            }
            if let Some(m) = max_severity {
                prop_assert!(inc.severity() >= m);
            }
        }
    }
}
