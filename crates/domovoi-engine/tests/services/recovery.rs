//! What the service that has only just started does with actions the store already
//! holds: one approved but never dispatched is started, one left running is failed
//! with the reason that says the host must be checked (ADR 0014).

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{ActionRepo, IncidentRepo, Presence};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, PlanHash, Principal, SignalStatus, UserId,
};
use domovoi_engine::actions::audit;
use domovoi_engine::{
    ActionDeps, ActionService, ActionSettings, EnginePolicy, IncidentManager, ManagerDeps,
    PatternRedactor,
};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::SignedDuration;
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::actions_harness::{Actions, check, ok_output, verified_restart};
use crate::harness::{SECRET, host_id};

/// Why a run the previous session left running is failed. The test spells it out so a
/// reader sees the exact sentence the person is shown.
const RESTART: &str = "the hub restarted while this ran; check the host before running it again";

/// What `restart_container` needs to restart `nginx`.
fn restart_params() -> BTreeMap<String, String> {
    let pairs: [(&str, &str); 1] = [("container", "nginx")];
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// A world whose store already holds an action the session that just stopped left
/// `Approved` (`dispatched` false) or `Dispatched`, and whose service starts only
/// after that, the way the next hub session finds the store on disk.
async fn restarted(dispatched: bool) -> (Actions, ActionId) {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    link.set_skills(host_id(), vec![verified_restart()]);
    link.on_skill(|_, _| Ok(ok_output("restarted nginx")));
    link.on_check(|_, _, _| Ok(check(SignalStatus::Resolved)));
    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();

    let skill = verified_restart();
    let params = restart_params();
    let validated = skill.validate_args(&params).unwrap();
    let mut action = ActionRequest::propose(
        incident.id(),
        host_id(),
        &skill,
        &validated,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    );
    action.approve(human(UserId::new()), T0).unwrap();
    if dispatched {
        let plan = PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &validated);
        action.dispatch(plan, T0).unwrap();
        assert!(
            action.status() == &ActionStatus::Dispatched,
            "the plan the agent has now is the approved one, so the previous session ran it",
        );
    }
    let id = action.id();
    ActionRepo::insert(&*store, &action).await.unwrap();

    // The service starts last: nothing here published the approval or the dispatch.
    let (bus, events) = broadcast::channel(64);
    let shutdown = CancellationToken::new();
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let (service, _task) = ActionService::spawn(
        ActionDeps {
            actions: store.clone(),
            incidents: store.clone(),
            audit: store.clone(),
            link: link.clone(),
            redactor: Arc::new(PatternRedactor::new([SecretString::from(
                SECRET.to_owned(),
            )])),
            clock: clock.clone(),
            manager,
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        },
        ActionSettings::default(),
    );
    (
        Actions {
            store,
            clock,
            link,
            service,
            events,
            bus,
            incident,
            shutdown,
        },
        id,
    )
}

#[tokio::test]
async fn an_action_approved_before_a_restart_runs_after_it() {
    let (a, id) = restarted(false).await;
    // The approval was made by the session that stopped, so no `ActionApproved` event
    // is on the bus: only reading the store finds the action still waiting.
    let action = a
        .until(id, |status| status == &ActionStatus::Verified)
        .await;
    assert!(
        action.status() == &ActionStatus::Verified,
        "the approval made before the restart ran the skill and re-checked it",
    );
    assert_eq!(
        a.runs(),
        1,
        "the run the store said was waiting reached the agent once",
    );

    let trail = a.trail(id).await;
    assert_eq!(
        trail
            .iter()
            .filter(|kind| *kind == audit::DISPATCHED)
            .count(),
        1,
        "the dispatch is recorded once",
    );
}

#[tokio::test]
async fn a_dispatched_action_fails_after_a_restart() {
    let (a, id) = restarted(true).await;
    a.settle().await;

    let stored = a.get(id).await;
    let ActionStatus::Failed { reason } = stored.status().clone() else {
        panic!("the restart left an action it cannot still be waiting for")
    };
    assert_eq!(
        reason,
        RESTART.to_owned(),
        "the failure says the host must be checked before running again",
    );
    assert_eq!(
        a.runs(),
        0,
        "a run the new session did not start is never started again",
    );

    let trail = a.trail(id).await;
    assert_eq!(
        trail,
        [audit::FAILED.to_owned()],
        "the leftover run is audited once, and nothing else is audited for it",
    );
}
