//! Expiry before dispatch, on every route but the writer's own dispatch guard (that one is
//! a unit test in `src/actions/freshness.rs`, where the writer can be driven directly).
//!
//! Every case asserts the same four facts: the stored status is `NotRun` with the expiry
//! reason; there is exactly one `action.not_run` audit entry and no `action.rejected`,
//! `action.failed` or `action.expired`; no request reached the agent; and the status never
//! counts as a run.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionGuard, ActionRepo, HostAvailability, Presence, RAN_STATUSES, StateWrite,
};
use domovoi_core::{
    ActionRequest, ActionStatus, ApprovalProvenance, Authorization, IncidentId, PlanBinding,
    Principal, UserId,
};
use domovoi_engine::ActionSettings;
use domovoi_engine::actions::audit;
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use jiff::{SignedDuration, Timestamp};

use crate::actions_harness::{Actions, Seeded, seeded, verified_restart};
use crate::harness::host_id;
use crate::uow_probe::RecordingUow;

/// The sentence the person is shown; the test spells it out.
const EXPIRED: &str = "the approval expired before the action was dispatched";

/// What `restart_container` needs to restart `nginx`.
fn restart_params() -> BTreeMap<String, String> {
    let pairs: [(&str, &str); 1] = [("container", "nginx")];
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// The v2 target the harness incident describes: `docker.container` on `nginx`.
/// An approved `restart_container` on `incident`, open for `ttl`.
fn approved_for(incident: IncidentId, ttl: SignedDuration) -> ActionRequest {
    let skill = verified_restart();
    let validated = skill.validate_args(&restart_params()).unwrap();
    let incident = domovoi_testkit::build::incident(host_id(), "docker.container", Some("nginx"))
        .expect("a valid incident")
        .with_id(incident);
    let mut action = ActionRequest::propose(
        &incident,
        &skill,
        &validated,
        Some(domovoi_testkit::build::key_fingerprint()),
        Principal::System("triage".into()),
        ttl,
        T0,
    )
    .unwrap();
    action
        .approve(
            Authorization::human(human(UserId::new())),
            ApprovalProvenance::new(None, "test"),
            &PlanBinding::Full(action.plan_hash_for_approval()),
            T0,
        )
        .unwrap();
    action
}

/// Stores `action` as the session that approved it left it.
async fn insert(world: &Seeded, action: &ActionRequest) {
    ActionRepo::insert(&*world.store, action).await.unwrap();
}

/// The host is gone and the hub has never heard from it.
fn unknown(world: &Seeded) {
    world
        .link
        .set_presence(host_id(), Presence::Disconnected { last_seen: None });
    world
        .link
        .set_availability(host_id(), HostAvailability::Unknown);
}

/// The host reconnects now.
fn connect(world: &Seeded, at: Timestamp) {
    world.link.clear_availability(host_id());
    world.link.set_presence(
        host_id(),
        Presence::Connected {
            since: at,
            version: "0.1.0".to_owned(),
        },
    );
}

/// Asserts the four facts every expiry route shares.
async fn settled_as_not_run(a: &Actions, id: domovoi_core::ActionId) {
    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::NotRun {
            reason: EXPIRED.to_owned(),
        },
        "a closed window is positive proof the run never started",
    );
    assert_eq!(
        a.trail(id).await,
        [audit::NOT_RUN.to_owned()],
        "the expiry is audited as a run that never happened, once",
    );
    assert_eq!(
        a.journal_reason(id).await.as_deref(),
        Some("expired"),
        "the journal says why the action ended",
    );
    assert_eq!(a.runs(), 0, "nothing reached the agent");
    assert!(
        !RAN_STATUSES.contains(&"not_run"),
        "a run that never started is not a run for the loop guard",
    );
}

#[tokio::test]
async fn approved_expiry_before_dispatch_is_not_run() {
    // The approval's window closed while the hub was down, so the run's first preflight
    // is what resolves it.
    let world = seeded().await;
    let action = approved_for(world.incident.id(), SignedDuration::from_hours(1));
    let id = action.id();
    insert(&world, &action).await;
    world.clock.advance(SignedDuration::from_hours(2));
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.until(id, |status| matches!(status, ActionStatus::NotRun { .. }))
        .await;

    settled_as_not_run(&a, id).await;
}

#[tokio::test]
async fn approved_expiry_during_the_host_wait_is_not_run() {
    // The host is absent at the first preflight and appears only after the window closed,
    // so the second preflight resolves the action.
    let world = seeded().await;
    let action = approved_for(world.incident.id(), SignedDuration::from_hours(1));
    let id = action.id();
    insert(&world, &action).await;
    unknown(&world);
    let settings = ActionSettings {
        offline_wait: SignedDuration::from_hours(3),
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());
    a.clock.advance(SignedDuration::from_secs(1));
    a.settle().await;
    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Approved,
        "the run waits for the host without deciding anything",
    );

    a.clock.advance(SignedDuration::from_mins(61));
    connect(&world, a.now());
    a.until(id, |status| matches!(status, ActionStatus::NotRun { .. }))
        .await;

    settled_as_not_run(&a, id).await;
}

#[tokio::test]
async fn expiry_sweep_resolves_an_approved_action_without_a_host() {
    // The run never starts: the action is stored after the executor's catch-up read the
    // store, so the sweep is the only writer that can resolve it.
    let world = seeded().await;
    let a = world.start(ActionSettings::default(), world.store.clone());
    a.settle().await;
    let action = approved_for(world.incident.id(), SignedDuration::from_hours(1));
    let id = action.id();
    insert(&world, &action).await;

    a.clock.advance(SignedDuration::from_mins(61));
    a.settle().await;
    assert_eq!(
        a.service.expire_due().await.unwrap(),
        1,
        "the sweep resolves the approval whose window closed",
    );

    settled_as_not_run(&a, id).await;

    // The agent arrives afterwards: the executor may not revive a settled action.
    connect(&world, a.now());
    a.settle().await;
    assert!(
        matches!(a.get(id).await.status(), ActionStatus::NotRun { .. }),
        "a settled action stays settled when its agent arrives",
    );
    assert_eq!(a.runs(), 0, "and the skill still never ran");
}

#[tokio::test]
async fn proposed_expiry_stays_expired() {
    let world = seeded().await;
    let a = world.start(ActionSettings::default(), world.store.clone());
    a.settle().await;
    let (proposal, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_mins(10),
        )
        .await
        .unwrap();

    a.clock.advance(SignedDuration::from_mins(11));
    assert_eq!(
        a.service.expire_due().await.unwrap(),
        1,
        "a proposal nobody decided still expires",
    );

    assert_eq!(
        a.get(proposal.id()).await.status(),
        &ActionStatus::Expired,
        "the proposal's window closing is still `Expired`",
    );
    assert_eq!(
        a.trail(proposal.id()).await,
        [audit::PROPOSED.to_owned(), audit::EXPIRED.to_owned()],
        "and it is audited as an expiry of the card, not as a run",
    );
    assert_eq!(a.runs(), 0, "a proposal never reaches the agent");
}

#[tokio::test]
async fn executor_never_writes_an_expiry_outcome() {
    // The executor only queues the decision: one commit, by the writer, guarded on
    // `approved` and carrying the `action.not_run` audit line.
    let world = seeded().await;
    let action = approved_for(world.incident.id(), SignedDuration::from_hours(1));
    let id = action.id();
    insert(&world, &action).await;
    world.clock.advance(SignedDuration::from_hours(2));
    let probe = Arc::new(RecordingUow::new(world.store.clone()));
    let a = world.start(ActionSettings::default(), probe.clone());

    a.until(id, |status| matches!(status, ActionStatus::NotRun { .. }))
        .await;
    a.settle().await;

    let commits = probe.commits();
    assert_eq!(commits.len(), 1, "the outcome is one commit");
    let StateWrite::Action { expected, .. } = &commits[0].writes[0] else {
        panic!("the one commit is the action's own write");
    };
    assert_eq!(
        expected,
        &ActionGuard::Status("approved".to_owned()),
        "it is guarded on the status the route read",
    );
    assert_eq!(commits[0].audit[0].kind, audit::NOT_RUN, "and audited once",);
}

#[tokio::test]
async fn a_rejection_queued_after_the_window_closed_is_not_run() {
    // The agent never came back, so the executor queues its "agent offline" rejection after
    // waiting out `offline_wait`. By then the approval window has closed: nothing was
    // attempted, so the writer settles it as a run that never happened.
    let world = seeded().await;
    let action = approved_for(world.incident.id(), SignedDuration::from_mins(1));
    let id = action.id();
    insert(&world, &action).await;
    unknown(&world);
    let settings = ActionSettings {
        offline_wait: SignedDuration::from_mins(5),
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());

    a.until(id, |status| matches!(status, ActionStatus::NotRun { .. }))
        .await;

    settled_as_not_run(&a, id).await;
    assert!(
        !a.trail(id).await.contains(&audit::REJECTED.to_owned()),
        "the reason the rejection was queued with is not what stopped it",
    );
}

#[tokio::test]
async fn legacy_proposed_action_expires_through_the_existing_sweep() {
    // A row written before the upgrade carries the old one-hour window; nothing creates one
    // now, and the sweep settles it by itself.
    let a = crate::actions_harness::actions(ActionSettings::default()).await;
    let (legacy, _) = a.propose().await;

    a.clock.advance(SignedDuration::from_mins(59));
    assert_eq!(a.service.expire_due().await.unwrap(), 0, "still inside it");
    a.clock.advance(SignedDuration::from_mins(2));
    assert_eq!(a.service.expire_due().await.unwrap(), 1);

    assert_eq!(a.get(legacy.id()).await.status(), &ActionStatus::Expired);
    assert_eq!(
        a.trail(legacy.id()).await,
        [audit::PROPOSED.to_owned(), audit::EXPIRED.to_owned()],
    );
    assert_eq!(a.runs(), 0);
}

#[tokio::test]
async fn legacy_approved_action_still_dispatches_with_orchestration_on() {
    // The legacy executor does not depend on `[orchestration]`: an approved row dispatches
    // once under its own id.
    let a = crate::actions_harness::actions(ActionSettings::default()).await;
    let (legacy, _) = a.propose().await;
    let plan = a.get(legacy.id()).await.plan_hash_for_approval();
    a.service
        .approve(
            legacy.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(plan),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();

    let done = a
        .until(legacy.id(), |s| matches!(s, ActionStatus::Verified))
        .await;
    assert_eq!(done.id(), legacy.id());
    assert_eq!(a.runs(), 1, "dispatched exactly once");
}
