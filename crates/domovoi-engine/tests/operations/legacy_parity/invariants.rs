//! INV-1 on the operation path: only a person's authorization lets a step run, and nothing a
//! bus event says changes that.

use domovoi_core::ports::EffectStatus;
use domovoi_core::{ExecutionId, Incident, OperationId, StepId, StepOutcome};
use domovoi_engine::{DomainEvent, EventKind};
use domovoi_testkit::approvals;
use domovoi_testkit::build::open_incident;

use crate::harness::{World, host_id, world};

/// The window an approval stays open, in seconds, plus one.
const PAST_WINDOW: i64 = 24 * 3600 + 1;

/// How many dispatch effects for `op` are neither done nor dead.
async fn open_dispatches(w: &World, op: OperationId) -> usize {
    let mut count = 0;
    for status in [EffectStatus::Pending, EffectStatus::Claimed] {
        count += w
            .effects(op, status)
            .await
            .iter()
            .filter(|effect| effect.kind == "dispatch")
            .count();
    }
    count
}

/// Publishes every event that has ever announced an approval or a change of one, for the step
/// `step`, as if a faulty or hostile publisher had made them.
fn forge_events(w: &World, incident: &Incident, step: StepId) {
    let execution = ExecutionId::from_step(step);
    for kind in [
        EventKind::ExecutionChanged { execution },
        EventKind::TriageRequested,
    ] {
        // No subscriber is a normal state; the point is that nobody acts on what is sent.
        let _ = w.bus.send(DomainEvent::new(kind, incident, true));
    }
}

/// Lets the service work and the clock run for a while.
async fn run_on(w: &World, seconds: i64) {
    for _ in 0..seconds {
        w.settle().await;
        w.advance(1);
    }
    w.settle().await;
}

#[tokio::test]
async fn only_approve_authorizes_a_step() {
    let incident = open_incident(host_id(), "docker.container", Some("plex")).unwrap();

    // A proposed step and a denied one: no event makes either of them run.
    let w = world().await;
    let op = w.create().await;
    let proposed = w.propose(op).await;
    let denied = w.propose(op).await;
    w.handle()
        .deny(op, denied, approvals::human(w.user))
        .await
        .unwrap();
    for step in [proposed, denied] {
        forge_events(&w, &incident, step);
    }
    run_on(&w, 60).await;
    assert_eq!(w.outcome(op, proposed).await, StepOutcome::Pending);
    assert_eq!(w.outcome(op, denied).await, StepOutcome::Refused);
    assert!(w.skill_requests().is_empty(), "no run went out");
    assert_eq!(open_dispatches(&w, op).await, 0, "no dispatch was queued");
    assert!(
        !w.audit_kinds()
            .await
            .contains(&"execution.approved".to_owned()),
        "nothing was approved"
    );

    // A step whose window closed: no event brings it back.
    let w = world().await;
    let op = w.create().await;
    let expiring = w.propose(op).await;
    w.advance(PAST_WINDOW);
    run_on(&w, 60).await;
    assert_eq!(w.outcome(op, expiring).await, StepOutcome::NotRun);
    forge_events(&w, &incident, expiring);
    run_on(&w, 60).await;
    assert_eq!(w.outcome(op, expiring).await, StepOutcome::NotRun);
    assert!(w.skill_requests().is_empty(), "no run went out");
    assert_eq!(open_dispatches(&w, op).await, 0, "no dispatch was queued");
    assert!(
        !w.audit_kinds()
            .await
            .contains(&"execution.approved".to_owned()),
        "nothing was approved"
    );
}

// `rejected_or_failed_steps_never_retry_silently` (INV-6) lives in `dispatch.rs`: a failed and
// a refused step stay as they are however long the service runs, and only a new proposal
// creates a new dispatch.
