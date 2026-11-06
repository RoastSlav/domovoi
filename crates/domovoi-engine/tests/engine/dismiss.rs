//! Dismissing an incident as a known condition.

use domovoi_core::{
    AckRule, AckRuleId, CheckKey, HostId, IncidentId, IncidentStatus, TargetPattern, UserId,
    Verdict,
};
use domovoi_engine::manager::{Command, CommandError, Decision, Write};
use domovoi_engine::{EnginePolicy, EventKind};
use domovoi_testkit::build::at;

use crate::harness::{Sig, Sim, kinds};

use EventKind::IncidentResolved;

const REASON: &str = "the nightly backup fills the disk and the job cleans it up";

fn dismiss(sim: &mut Sim, id: IncidentId, secs: i64) -> Decision {
    sim.run(
        Command::Dismiss {
            incident: id,
            by: UserId::new(),
            reason: REASON.to_owned(),
        },
        secs,
    )
}

#[test]
fn dismiss_closes_the_incident_with_the_expected_verdict() {
    let mut sim = Sim::new(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    let id = sim.only_active().id();

    let decision = dismiss(&mut sim, id, 100);
    assert_eq!(kinds(&decision), [IncidentResolved]);
    assert!(sim.state.active.is_empty(), "the incident is not active");

    let closed = &sim.state.resolved[&disk.firing(0).fingerprint()];
    assert_eq!(closed.status(), IncidentStatus::Resolved);
    assert_eq!(closed.resolved_at(), Some(at(100)));
    assert_eq!(closed.verdict(), Some(Verdict::Expected));

    let entry = decision
        .writes
        .iter()
        .find(|w| matches!(w, Write::Timeline(e) if e.kind == "human.dismissed"))
        .expect("the dismiss is recorded in the timeline");
    let Write::Timeline(entry) = entry else {
        unreachable!()
    };
    assert_eq!(entry.incident, id);
    assert_eq!(entry.at, at(100));
    assert!(
        entry.summary.contains("dismissed by user:") && entry.summary.contains(REASON),
        "the summary is a plain sentence: {}",
        entry.summary
    );
    assert_eq!(entry.detail["reason"], REASON);
}

#[test]
fn dismissing_an_incident_that_is_not_active_is_unknown() {
    let mut sim = Sim::new(EnginePolicy::default());
    let disk = Sig::new(HostId::new(), "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    sim.ingest(disk.resolved(100), 100);
    let id = sim.state.resolved[&disk.firing(0).fingerprint()].id();

    let err = sim
        .try_run(
            Command::Dismiss {
                incident: id,
                by: UserId::new(),
                reason: "not a real problem".to_owned(),
            },
            200,
        )
        .unwrap_err();
    assert!(
        matches!(err, CommandError::UnknownIncident(_)),
        "a resolved incident cannot be dismissed: {err:?}"
    );
}

#[test]
fn known_condition_alerts_quietly_after_a_dismiss_rule() {
    let mut sim = Sim::new(EnginePolicy::default());
    let host = HostId::new();
    let disk = Sig::new(host, "disk.free", Some("/")).immediate();
    sim.ingest(disk.firing(0), 0);
    let id = sim.only_active().id();

    let decision = dismiss(&mut sim, id, 100);
    assert_eq!(kinds(&decision), [IncidentResolved]);

    // The hub creates the rule for the dismissed check, and the check fires again.
    sim.state.acks.push(AckRule {
        id: AckRuleId::new(),
        host: Some(host),
        check: CheckKey::parse("disk.free").unwrap(),
        target_pattern: Some(TargetPattern::parse("/").unwrap()),
        predicates: Vec::new(),
        note: REASON.to_owned(),
        expires_at: None,
    });
    let again = sim.ingest(disk.firing(200), 200);
    assert!(
        again.events.iter().all(|e| !e.notify),
        "a known condition must not alert: {:?}",
        kinds(&again)
    );
    assert_eq!(
        sim.state.resolved[&disk.firing(0).fingerprint()].verdict(),
        Some(Verdict::Expected),
        "the verdict stays EXPECTED, so grouping keeps it as handled"
    );
}
