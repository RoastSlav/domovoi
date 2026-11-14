//! What the service that has only just started does with actions the store already
//! holds: one approved but never dispatched is started, one left running is failed
//! with the reason that says the host must be checked (ADR 0014), and the loop guard
//! remembers what the previous session ran in the hour it is still in.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AuditFilter, AuditLog, HostAvailability, IncidentRepo, Page, Presence,
};
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

/// What the session that just stopped left an action as in the store.
enum LeftBehind {
    /// Still waiting for the new session to start it.
    Approved,
    /// Running when the hub stopped; its answer went to the session that is gone.
    Dispatched,
    /// Run to the end by that session, inside the hour the new session is still in, so
    /// the loop guard has to remember it from the audit log.
    Ran,
}

/// What `restart_container` needs to restart `nginx`.
fn restart_params() -> BTreeMap<String, String> {
    let pairs: [(&str, &str); 1] = [("container", "nginx")];
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// A world whose store already holds one action per state in `states`, left behind by the
/// session that just stopped, in the given order; ids come back in that same order. A
/// `Ran` action also carries the audit row that session wrote for it, inside the hour the
/// new session is in. The service starts only after the seeding, the way the next hub
/// session finds the store on disk.
async fn restarted(states: Vec<LeftBehind>) -> (Actions, Vec<ActionId>) {
    restarted_with(states, |_, _| {}).await
}

/// Like [`restarted`], but `set_up` runs after the host and its agent are scripted and
/// before the service starts, so a test can leave the host unreachable, scripted as
/// incompatible, or with the clock already past an approval's window.
async fn restarted_with(
    states: Vec<LeftBehind>,
    set_up: impl FnOnce(&Arc<FakeClock>, &Arc<FakeAgentLink>),
) -> (Actions, Vec<ActionId>) {
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
    set_up(&clock, &link);
    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();

    let skill = verified_restart();
    let params = restart_params();
    let validated = skill.validate_args(&params).unwrap();
    let mut ids = Vec::new();
    for (index, state) in states.iter().enumerate() {
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
        if matches!(state, LeftBehind::Dispatched | LeftBehind::Ran) {
            let plan =
                PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &validated);
            action.dispatch(plan, T0).unwrap();
            assert!(
                action.status() == &ActionStatus::Dispatched,
                "the plan the agent has now is the approved one, so the previous session ran it",
            );
        }
        if matches!(state, LeftBehind::Ran) {
            action.complete(Ok(()), T0).unwrap();
            assert!(
                action.status() == &ActionStatus::Succeeded,
                "the run the previous session made finished",
            );
            // That session audited the dispatch it made, and the instant it wrote is
            // inside the hour the new session is still in, so the guard has to read it.
            let ran_at = T0
                .checked_sub(SignedDuration::from_mins(40 - 10 * index as i64))
                .unwrap();
            AuditLog::append(&*store, &audit::dispatched(&action, ran_at).into_event())
                .await
                .unwrap();
        }
        ActionRepo::insert(&*store, &action).await.unwrap();
        ids.push(action.id());
    }

    // The service starts last: nothing here published the approval or the dispatch.
    let (bus, events) = broadcast::channel(64);
    let shutdown = CancellationToken::new();
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
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
            uow: store.clone(),
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
        ids,
    )
}

#[tokio::test]
async fn an_action_approved_before_a_restart_runs_after_it() {
    let (a, ids) = restarted(vec![LeftBehind::Approved]).await;
    let id = ids[0];
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
        "the run the store said was waiting reached the agent once"
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
async fn startup_recovery_runs_before_catch_up() {
    // One action is left running and one is still approved. The new session must fail the
    // running one before it dispatches the waiting one, or a look at the audit log could
    // show a dispatch that the recovery then contradicts.
    let (a, ids) = restarted(vec![LeftBehind::Dispatched, LeftBehind::Approved]).await;
    a.until(ids[1], |status| status == &ActionStatus::Verified)
        .await;

    let mut entries = AuditLog::list(&*a.store, &AuditFilter::default(), &Page::first(100))
        .await
        .unwrap()
        .items;
    entries.reverse();
    let position = |id: ActionId, kind: &str| {
        let subject = format!("action:{id}");
        entries
            .iter()
            .position(|entry| entry.subject == subject && entry.kind == kind)
    };
    let failed = position(ids[0], audit::FAILED).expect("the leftover run was failed");
    let dispatched =
        position(ids[1], audit::DISPATCHED).expect("the waiting approval was dispatched");
    assert!(
        failed < dispatched,
        "recovery failed the leftover run before catch-up dispatched the waiting one",
    );
}

#[tokio::test]
async fn a_dispatched_action_fails_after_a_restart() {
    let (a, ids) = restarted(vec![LeftBehind::Dispatched]).await;
    let id = ids[0];
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
        "a run the new session did not start is never started again"
    );

    let trail = a.trail(id).await;
    assert_eq!(
        trail,
        [audit::FAILED.to_owned()],
        "the leftover run is audited once, and nothing else is audited for it",
    );
}

#[tokio::test]
async fn loop_guard_remembers_the_previous_sessions_runs_across_a_restart() {
    // Three runs of the same skill on the same host, all inside the hour the new session
    // is in, and one approval still waiting when the hub stopped.
    let (a, ids) = restarted(vec![
        LeftBehind::Ran,
        LeftBehind::Ran,
        LeftBehind::Ran,
        LeftBehind::Approved,
    ])
    .await;
    let pending = ids[3];
    let guarded = a
        .until(pending, |status| {
            matches!(status, ActionStatus::Rejected { .. })
        })
        .await;
    assert!(
        matches!(
            guarded.status(),
            ActionStatus::Rejected { reason } if reason.starts_with("loop guard")
        ),
        "the three runs the previous session made still count, so the pending approval is stopped before it runs",
    );
    assert_eq!(
        a.runs(),
        0,
        "nothing is run: the guard stopped the pending approval, and the runs the previous session made are not started again",
    );
    assert_eq!(
        a.trail(pending).await,
        [audit::BLOCKED.to_owned()],
        "the pending approval is stopped by the guard, and nothing reached the agent for it",
    );
    let mut guarded_runs = 0;
    for id in ids.iter().take(3) {
        if a.trail(*id)
            .await
            .iter()
            .any(|kind| *kind == audit::DISPATCHED)
        {
            guarded_runs += 1;
        }
    }
    assert_eq!(
        guarded_runs, 3,
        "the audit log still holds the three dispatches the previous session made, which is what the guard reads back",
    );
}

#[tokio::test]
async fn approvals_caught_up_at_once_share_one_loop_guard() {
    // Four approvals the previous session left, all the same skill on the same host, are
    // started together by the new session: the hour only allows three runs.
    let (a, ids) = restarted(vec![
        LeftBehind::Approved,
        LeftBehind::Approved,
        LeftBehind::Approved,
        LeftBehind::Approved,
    ])
    .await;
    let mut done = Vec::new();
    for id in ids {
        let action = a
            .until(id, |status| {
                matches!(
                    status,
                    ActionStatus::Verified | ActionStatus::Rejected { .. }
                )
            })
            .await;
        done.push(action.status().clone());
    }
    let ran = done
        .iter()
        .filter(|status| *status == &ActionStatus::Verified)
        .count();
    assert_eq!(
        ran, 3,
        "the hour allows three runs of this skill on this host, no matter how many approvals are caught up at once",
    );
    assert_eq!(
        a.runs(),
        3,
        "three skills reached the agent, and the guard stopped the fourth before it was dispatched",
    );
    let guarded = done
        .iter()
        .filter(
            |status| matches!(status, ActionStatus::Rejected { reason } if reason.starts_with("loop guard")),
        )
        .count();
    assert_eq!(
        guarded, 1,
        "one of the approvals caught up at once is stopped, and its reason is the guard",
    );
}

#[tokio::test]
async fn catch_up_waits_for_unknown_host() {
    // The hub just restarted and its agent server has not heard from this host yet, so
    // the host is unknown rather than absent: the approval waits for it instead of
    // failing, however long the agent takes to reconnect.
    let (a, ids) = restarted_with(vec![LeftBehind::Approved], |_, link| {
        link.set_presence(host_id(), Presence::Disconnected { last_seen: None });
        link.set_availability(host_id(), HostAvailability::Unknown);
    })
    .await;
    let id = ids[0];
    // Well inside the 30-minute wait, so the point is that it is still waiting rather
    // than that the wait has a limit.
    for _ in 0..4 {
        a.clock.advance(SignedDuration::from_mins(5));
        a.settle().await;
    }
    let action = a.get(id).await;
    assert!(
        action.status() == &ActionStatus::Approved,
        "an approval whose host has not reconnected yet is still waiting, not failed: {:?}",
        action.status(),
    );
    assert_eq!(a.runs(), 0, "nothing reached an agent that is not there",);
}

#[tokio::test]
async fn stale_approval_not_dispatched_after_restart() {
    // The approval's window closed while the hub was down: the new session may not run it.
    let (a, ids) = restarted_with(vec![LeftBehind::Approved], |clock, _| {
        clock.advance(SignedDuration::from_hours(2));
    })
    .await;
    let id = ids[0];
    let action = a
        .until(id, |status| matches!(status, ActionStatus::Rejected { .. }))
        .await;
    assert_eq!(
        action.status(),
        &ActionStatus::Rejected {
            reason: "the approval expired before the action was dispatched".to_owned(),
        },
        "the action whose approval expired is refused with the reason the person can act on",
    );
    assert_eq!(
        a.runs(),
        0,
        "an approval that expired while the hub was down never reaches the agent",
    );
}

#[tokio::test]
async fn loop_guard_reloads_dispatches_that_were_committed_with_their_audit_rows() {
    // The first session dispatches three runs, each committed with its audit row; the
    // second session starts over the same store and must count all three.
    let world = crate::actions_harness::seeded().await;
    let settings = ActionSettings::default();
    let first = world.start(settings, world.store.clone());
    for _ in 0..3 {
        let (action, _) = first
            .propose_via_service(
                &verified_restart(),
                &[("container", "nginx")],
                SignedDuration::from_hours(1),
            )
            .await
            .unwrap();
        first
            .service
            .approve(action.id(), human(UserId::new()), None)
            .await
            .unwrap();
        first
            .until(action.id(), |status| status == &ActionStatus::Verified)
            .await;
    }
    let runs = first.runs();
    assert_eq!(runs, 3, "the first session ran the skill three times");
    drop(first);

    let second = world.start(settings, world.store.clone());
    let (fourth, _) = second
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    second
        .service
        .approve(fourth.id(), human(UserId::new()), None)
        .await
        .unwrap();
    let done = second
        .until(fourth.id(), |status| {
            matches!(status, ActionStatus::Rejected { .. })
        })
        .await;
    assert!(
        matches!(done.status(), ActionStatus::Rejected { reason } if reason.starts_with("loop guard")),
        "the dispatches committed with their audit rows still count after a restart",
    );
    assert_eq!(
        second.runs(),
        runs,
        "the guard stopped the fourth run before it reached the agent",
    );
}
