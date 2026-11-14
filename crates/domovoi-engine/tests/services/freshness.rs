//! What an approved action does while its host is not there, and what stops it from
//! running when it is no longer wanted: the wait for the agent, the re-check of the
//! approval and incident before dispatch, and the catch-up pool that keeps a restart from
//! flooding the agents.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionRepo, AgentLink, CheckRecheck, Clock, HostAvailability, IncidentRepo, LinkError,
    Presence, SkillDispatch, SkillOutput, ToolOutput,
};
use domovoi_core::skill::{ParamKind, ParamName, ParamSpec};
use domovoi_core::{
    ActionRequest, ActionStatus, CheckKey, HostId, IncidentEvent, IncidentId, IncidentPolicy,
    Principal, ReadToolCall, Risk, SkillSummary, TargetKey, UserId, apply,
};
use domovoi_engine::actions::audit;
use domovoi_engine::{ActionDeps, ActionService, ActionSettings, PatternRedactor};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::{FakeAgentLink, FakeClock};
use jiff::SignedDuration;
use secrecy::SecretString;

use crate::actions_harness::{Actions, Seeded, seeded, verified_restart};
use crate::harness::{SECRET, host_id};

/// The arguments `restart_container` is approved with.
fn restart_params() -> BTreeMap<String, String> {
    let pairs: [(&str, &str); 1] = [("container", "nginx")];
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// An approved `restart_container` for `host` on `incident`, open for `ttl`.
fn approved_for(host: HostId, incident: IncidentId, ttl: SignedDuration) -> ActionRequest {
    let skill = verified_restart();
    let validated = skill.validate_args(&restart_params()).unwrap();
    let mut action = ActionRequest::propose(
        incident,
        host,
        &skill,
        &validated,
        Principal::System("triage".into()),
        ttl,
        T0,
    );
    action.approve(human(UserId::new()), T0).unwrap();
    action
}

/// Stores `action` the way the session that approved it left it in the store.
async fn insert(world: &Seeded, action: &ActionRequest) {
    ActionRepo::insert(&*world.store, action).await.unwrap();
}

/// `restart_container` on a manifest that differs from the approved one, so the plan the
/// agent has now no longer matches what was approved while its arguments still validate.
fn changed_restart() -> SkillSummary {
    let mut manifest = domovoi_testkit::build::manifest("restart_container", Risk::Write).unwrap();
    manifest.description = "Restart a container, as announced by an updated agent.".to_owned();
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").unwrap(),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary()
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
fn connect(world: &Seeded, host: HostId, clock: &Arc<FakeClock>) {
    world.link.clear_availability(host);
    world.link.set_presence(
        host,
        Presence::Connected {
            since: clock.now(),
            version: "0.1.0".to_owned(),
        },
    );
}

/// Asserts an action was rejected with `reason` and never reached the agent.
fn rejected(done: &ActionRequest, reason: &str) {
    assert_eq!(
        done.status(),
        &ActionStatus::Rejected {
            reason: reason.to_owned(),
        },
        "the action is refused with the reason the person is shown",
    );
}

#[tokio::test]
async fn approval_for_resolved_incident_is_not_dispatched() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    let mut incident = world.incident.clone();
    apply(
        &mut incident,
        IncidentEvent::Resolved { observed_at: T0 },
        &IncidentPolicy::default(),
        T0,
    )
    .unwrap();
    IncidentRepo::update(&*world.store, &incident)
        .await
        .unwrap();

    let a = world.start(ActionSettings::default(), world.store.clone());
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(
        &done,
        "the incident was resolved before the action was dispatched",
    );
    assert_eq!(a.runs(), 0, "a resolved incident never reaches the agent");
    assert!(
        !a.trail(action.id())
            .await
            .contains(&audit::DISPATCHED.to_owned()),
        "nothing was dispatched",
    );
}

#[tokio::test]
async fn approval_for_deleted_incident_is_not_dispatched() {
    let world = seeded().await;
    let action = approved_for(host_id(), IncidentId::new(), SignedDuration::from_hours(1));
    insert(&world, &action).await;

    let a = world.start(ActionSettings::default(), world.store.clone());
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(&done, "the incident no longer exists");
    assert_eq!(a.runs(), 0, "an action without an incident never runs");
}

#[tokio::test]
async fn approval_expiring_while_waiting_for_host_is_not_dispatched() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    unknown(&world);
    let settings = ActionSettings {
        offline_wait: SignedDuration::from_hours(3),
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());
    // The approval's hour closes while the action waits.
    for _ in 0..12 {
        a.clock.advance(SignedDuration::from_mins(5));
        a.settle().await;
    }
    a.clock.advance(SignedDuration::from_mins(1));
    a.settle().await;
    // The agent comes back, but the approval is gone.
    connect(&world, host_id(), &a.clock);

    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(
        &done,
        "the approval expired before the action was dispatched",
    );
    assert_eq!(a.runs(), 0, "an expired approval never reaches the agent");
}

#[tokio::test]
async fn plan_changed_while_host_unknown_is_rejected() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    unknown(&world);
    let a = world.start(ActionSettings::default(), world.store.clone());
    // The agent comes back announcing the same skill with a different manifest.
    world.link.set_skills(host_id(), vec![changed_restart()]);
    connect(&world, host_id(), &a.clock);

    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(&done, "the plan changed after approval");
    assert_eq!(
        a.runs(),
        0,
        "a changed plan is refused before the agent runs it"
    );
}

#[tokio::test]
async fn skill_missing_after_host_connects_is_rejected_with_a_real_catalogue() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    unknown(&world);
    let a = world.start(ActionSettings::default(), world.store.clone());
    // The agent comes back with a real but shorter catalogue: the skill is gone.
    world.link.set_skills(host_id(), Vec::new());
    connect(&world, host_id(), &a.clock);

    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(&done, "the skill is no longer on the host");
    assert_eq!(a.runs(), 0, "a skill the agent no longer has never runs");
}

#[tokio::test]
async fn unknown_host_past_wait_limit_is_rejected_without_dispatch() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    unknown(&world);
    let a = world.start(ActionSettings::default(), world.store.clone());
    for _ in 0..7 {
        a.clock.advance(SignedDuration::from_mins(5));
        a.settle().await;
    }
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(&done, "agent offline");
    assert_eq!(a.runs(), 0, "a host that never came back ran nothing");
    assert!(
        !a.trail(action.id())
            .await
            .contains(&audit::DISPATCHED.to_owned()),
        "nothing was dispatched",
    );

    // The refusal took no loop-guard slot: with the agent back, a fresh approval runs.
    connect(&world, host_id(), &a.clock);
    let (fresh, _) = a.propose().await;
    a.service
        .approve(fresh.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.until(fresh.id(), |s| s == &ActionStatus::Verified).await;
    assert_eq!(a.runs(), 1, "the action that never dispatched used no slot");
}

#[tokio::test]
async fn absent_host_that_returns_in_time_runs_the_action() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    world.link.set_presence(
        host_id(),
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
    world
        .link
        .set_availability(host_id(), HostAvailability::Absent { since: T0 });
    let a = world.start(ActionSettings::default(), world.store.clone());
    a.clock.advance(SignedDuration::from_mins(10));
    a.settle().await;
    connect(&world, host_id(), &a.clock);

    let done = a.until(action.id(), |s| s == &ActionStatus::Verified).await;
    assert!(
        done.status() == &ActionStatus::Verified,
        "an agent that returns inside the wait still runs the approved action",
    );
    assert_eq!(a.runs(), 1, "the skill ran once");
}

#[tokio::test]
async fn incompatible_host_is_rejected_immediately() {
    let world = seeded().await;
    let action = approved_for(
        host_id(),
        world.incident.id(),
        SignedDuration::from_hours(1),
    );
    insert(&world, &action).await;
    world.link.set_availability(
        host_id(),
        HostAvailability::Incompatible {
            reported: "1.0".to_owned(),
        },
    );
    let a = world.start(ActionSettings::default(), world.store.clone());
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    rejected(
        &done,
        "agent upgrade required: the agent speaks another protocol version",
    );
    assert_eq!(
        a.now(),
        T0,
        "an incompatible agent is refused without waiting for it",
    );
    assert_eq!(a.runs(), 0, "nothing is sent to an agent this hub rejects");
}

#[tokio::test]
async fn waiting_runs_do_not_starve_other_hosts() {
    let world = seeded().await;
    let other = HostId::new();
    let other_incident = open_incident(other, "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*world.store, &other_incident)
        .await
        .unwrap();
    world.link.set_skills(other, vec![verified_restart()]);
    world.link.set_presence(
        other,
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );

    // Nine approvals on the host that is gone would fill the catch-up pool if the pool
    // were taken before the wait.
    let mut waiting = Vec::new();
    for _ in 0..9 {
        let action = approved_for(
            host_id(),
            world.incident.id(),
            SignedDuration::from_hours(1),
        );
        insert(&world, &action).await;
        waiting.push(action.id());
    }
    let ready = approved_for(other, other_incident.id(), SignedDuration::from_hours(1));
    insert(&world, &ready).await;
    unknown(&world);

    let a = world.start(ActionSettings::default(), world.store.clone());
    a.clock.advance(SignedDuration::from_mins(5));
    let done = a.until(ready.id(), |s| s == &ActionStatus::Verified).await;
    assert!(
        done.status() == &ActionStatus::Verified,
        "the host that is there runs while another host is waited for",
    );
    for id in waiting {
        assert!(
            a.get(id).await.status() == &ActionStatus::Approved,
            "an action waiting for its host is untouched by another host's run",
        );
    }
    assert_eq!(a.runs(), 1, "only the reachable host's skill ran");
}

/// An agent link whose `run_skill` records the request and then holds the run for `hold`
/// on the fake clock, so a test can see how many runs are in flight at once.
struct SlowLink {
    inner: Arc<FakeAgentLink>,
    clock: Arc<FakeClock>,
    hold: SignedDuration,
}

#[async_trait]
impl AgentLink for SlowLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: jiff::Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        self.inner.run_tool(host, call, deadline).await
    }

    async fn run_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        deadline: jiff::Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        let output = self.inner.run_skill(host, req, deadline).await;
        let until = self.clock.now().checked_add(self.hold).unwrap_or(deadline);
        self.clock.sleep_until(until).await;
        output
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: jiff::Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        self.inner.run_check(host, check, target, deadline).await
    }

    fn presence(&self, host: HostId) -> Presence {
        self.inner.presence(host)
    }

    async fn availability(&self, host: HostId) -> HostAvailability {
        AgentLink::availability(&*self.inner, host).await
    }

    fn skills(&self, host: HostId) -> Vec<domovoi_core::SkillSummary> {
        self.inner.skills(host)
    }
}

/// Starts the action service over `world` with a link that holds every run for half an
/// hour of fake time, so the catch-up pool can be observed.
fn slow_service(world: &Seeded, settings: ActionSettings) -> Actions {
    let link: Arc<dyn AgentLink> = Arc::new(SlowLink {
        inner: world.link.clone(),
        clock: world.clock.clone(),
        hold: SignedDuration::from_mins(30),
    });
    let shutdown = world.shutdown.child_token();
    let events = world.bus.subscribe();
    let (service, _task) = ActionService::spawn(
        ActionDeps {
            actions: world.store.clone(),
            incidents: world.store.clone(),
            audit: world.store.clone(),
            uow: world.store.clone(),
            link,
            redactor: Arc::new(PatternRedactor::new([SecretString::from(
                SECRET.to_owned(),
            )])),
            clock: world.clock.clone(),
            manager: world.manager.clone(),
            bus: world.bus.clone(),
            shutdown: shutdown.clone(),
        },
        settings,
    );
    Actions {
        store: world.store.clone(),
        clock: world.clock.clone(),
        link: world.link.clone(),
        service,
        events,
        bus: world.bus.clone(),
        incident: world.incident.clone(),
        shutdown,
    }
}

/// A world with `pending` approved actions on a reachable host, and the slow service.
async fn slow_world(pending: usize) -> (Actions, Vec<domovoi_core::ActionId>) {
    let world = seeded().await;
    let settings = ActionSettings {
        loop_guard_per_hour: 100,
        ..ActionSettings::default()
    };
    let mut ids = Vec::new();
    for _ in 0..pending {
        let action = approved_for(
            host_id(),
            world.incident.id(),
            SignedDuration::from_hours(1),
        );
        insert(&world, &action).await;
        ids.push(action.id());
    }
    let a = slow_service(&world, settings);
    (a, ids)
}

#[tokio::test]
async fn catch_up_dispatches_at_most_eight_at_once_once_hosts_are_known() {
    let (a, ids) = slow_world(12).await;
    a.settle().await;
    assert_eq!(
        a.runs(),
        8,
        "the catch-up pool lets eight runs reach the agent, not twelve",
    );
    let mut still_approved = 0;
    for id in &ids {
        if a.get(*id).await.status() == &ActionStatus::Approved {
            still_approved += 1;
        }
    }
    assert_eq!(
        still_approved, 4,
        "the approvals past the pool are still waiting, not dispatched",
    );

    a.clock.advance(SignedDuration::from_mins(31));
    for id in ids {
        a.until(id, |s| s == &ActionStatus::Verified).await;
    }
    assert_eq!(a.runs(), 12, "every approval still runs, eight at a time");
}

#[tokio::test]
async fn approved_action_is_still_started_by_the_event_path_without_the_pool() {
    let (a, _ids) = slow_world(8).await;
    a.settle().await;
    assert_eq!(a.runs(), 8, "the pool is full");

    // A new approval arrives while the pool is full; it is what the hub is doing now, so
    // it is not held to the catch-up pool.
    let (action, _) = a.propose().await;
    a.service
        .approve(action.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.settle().await;
    assert_eq!(
        a.runs(),
        9,
        "an approval that arrives as an event runs beside the catch-up runs",
    );
}
