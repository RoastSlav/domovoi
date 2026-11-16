//! Reconciling runs the hub dispatched but could no longer see the result of, when the
//! host's agent authenticates: its journal settles each run to what it recorded, to
//! `NotRun` when it has no record, or to `OutcomeUnknown` when the outcome cannot be
//! established. A run is never turned into a success or a failure without the journal
//! saying so, and nothing is written for a host with no session.
//!
//! These are the hub-side outcomes of the agent's journal crash windows; the agent side of
//! each window is tested with the journal itself, under the same names.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, ActionRepo, AuditLog, HostReconciler,
    LinkError, RAN_STATUSES, UnknownReason,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, IncidentId, JournalEpoch, PlanBinding, PlanHash,
    Principal, UserId,
};
use domovoi_engine::ActionSettings;
use domovoi_engine::actions::audit;
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;

use crate::actions_harness::{Actions, Seeded, seeded, verified_restart};
use crate::harness::host_id;
use crate::uow_probe::{RecordingUow, RefusingAuditUow};

/// The sentences the person is shown, spelled out so a reader sees them exactly.
const NOT_RUN: &str = "the agent has no record of this run; nothing was started";
const NO_START: &str = "the agent recorded this run but not its start; it may not have run";
const AGENT_RESTARTED: &str =
    "the agent restarted while this ran; check the host before running it again";
const JOURNAL_LOST: &str =
    "the agent lost its record of this run; check the host before running it again";
const EPOCH_CHANGED: &str =
    "the agent's journal was reset since this ran; the result cannot be recovered";
const EPOCH_UNKNOWN: &str =
    "this ran before the agent kept a journal; the result cannot be recovered";

/// What `restart_container` needs to restart `nginx`.
fn restart_params() -> BTreeMap<String, String> {
    let pairs: [(&str, &str); 1] = [("container", "nginx")];
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// A `restart_container` sent to the agent under `epoch`, left `Dispatched` by a hub
/// session that never saw the answer.
fn dispatched_for(incident: IncidentId, epoch: Option<JournalEpoch>) -> ActionRequest {
    let skill = verified_restart();
    let validated = skill.validate_args(&restart_params()).unwrap();
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &validated);
    let mut action = ActionRequest::propose(
        incident,
        host_id(),
        &skill,
        &validated,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    );
    action.approve(human(UserId::new()), T0).unwrap();
    action.dispatch(plan, epoch, T0).unwrap();
    action
}

/// Stores `action` as the session that sent it left it: the row and the audit line its
/// dispatch wrote.
async fn seed(world: &Seeded, action: &ActionRequest) {
    ActionRepo::insert(&*world.store, action).await.unwrap();
    AuditLog::append(&*world.store, &audit::dispatched(action, T0).into_event())
        .await
        .unwrap();
}

/// A live journal epoch for the host, the run the previous session left under it, and the
/// answer the agent gives about it. Returns the settled service and the run's id.
async fn reconciling(answer: ActionAnswer) -> (Actions, ActionId) {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    world.link.answer(host_id(), id, answer);
    let a = world.start(ActionSettings::default(), world.store.clone());
    a.service.reconcile_host(host_id()).await;
    (a, id)
}

/// The ids each `action_status` query carried, in order.
fn asked(a: &Actions) -> Vec<Vec<ActionId>> {
    a.link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Status { ids, .. } => Some(ids),
            _ => None,
        })
        .collect()
}

/// Lets the reconciliation pass run until it has asked `count` times.
async fn wait_for_queries(a: &Actions, count: usize) {
    for _ in 0..5000 {
        if asked(a).len() >= count {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("reconciliation never asked {count} times");
}

#[tokio::test]
async fn crash_after_accepted_is_outcome_unknown() {
    // The agent recorded the request but crashed before it recorded a start.
    let (a, id) = reconciling(ActionAnswer::Unknown {
        reason: UnknownReason::NoStartRecord,
    })
    .await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::OutcomeUnknown {
            reason: NO_START.to_owned(),
        },
        "uncertainty is never turned into success or failure",
    );
    assert_eq!(
        a.trail(id).await,
        [
            audit::DISPATCHED.to_owned(),
            audit::OUTCOME_UNKNOWN.to_owned()
        ],
        "the unknown outcome is audited once",
    );
    assert_eq!(
        a.journal_reason(id).await.as_deref(),
        Some("no_start_record"),
        "the journal keeps the machine-readable reason",
    );
    assert_eq!(a.runs(), 0, "reconciliation never runs a skill");
}

#[tokio::test]
async fn crash_after_started_is_outcome_unknown() {
    // The agent restarted while the run was recorded as started, so it may or may not
    // have finished.
    let (a, id) = reconciling(ActionAnswer::Unknown {
        reason: UnknownReason::AgentRestarted,
    })
    .await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::OutcomeUnknown {
            reason: AGENT_RESTARTED.to_owned(),
        },
        "an agent restart tells the person to check the host, never that it failed",
    );
    assert_eq!(
        a.journal_reason(id).await.as_deref(),
        Some("agent_restarted")
    );
    assert_eq!(a.runs(), 0, "reconciliation never runs a skill");
}

#[tokio::test]
async fn finished_durable_reply_lost_reconciles() {
    // The run finished and the journal has the result; the hub lost the reply.
    let (a, id) = reconciling(ActionAnswer::Finished {
        succeeded: true,
        detail: None,
    })
    .await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Succeeded,
        "the agent's recorded result is authoritative",
    );
    assert_eq!(
        a.trail(id).await,
        [audit::DISPATCHED.to_owned(), audit::SUCCEEDED.to_owned()],
        "the run is settled once, as the journal recorded it",
    );
    assert_eq!(a.journal_reason(id).await.as_deref(), Some("finished"));
    assert_eq!(a.runs(), 0, "reconciliation never runs a skill again");
}

#[tokio::test]
async fn finished_failure_carries_the_agents_message() {
    let (a, id) = reconciling(ActionAnswer::Finished {
        succeeded: false,
        detail: Some("the container is not there".to_owned()),
    })
    .await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Failed {
            reason: "the container is not there".to_owned(),
        },
        "a failure carries the agent's own message",
    );
    assert_eq!(a.runs(), 0, "reconciliation never runs a skill again");
}

#[tokio::test]
async fn hub_crash_before_commit_reconciles_from_journal() {
    // The pass gets the answer, the hub crashes before the commit lands, and the next
    // session asks again: the run is settled exactly once.
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    world.link.answer(
        host_id(),
        id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );

    let refusing = Arc::new(RefusingAuditUow::new(world.store.clone()));
    let crashed = world.start(ActionSettings::default(), refusing);
    crashed.service.reconcile_host(host_id()).await;
    assert!(
        matches!(
            crashed.get(id).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "a refused commit leaves the run where it was",
    );
    drop(crashed);

    let probe = Arc::new(RecordingUow::new(world.store.clone()));
    let restarted = world.start(ActionSettings::default(), probe.clone());
    restarted.service.reconcile_host(host_id()).await;
    assert_eq!(
        restarted.get(id).await.status(),
        &ActionStatus::Succeeded,
        "the next session asks the journal again and settles the run",
    );
    assert_eq!(
        probe.commits().len(),
        1,
        "exactly one commit settles the run, even after a refused one",
    );
    assert_eq!(
        restarted.trail(id).await,
        [audit::DISPATCHED.to_owned(), audit::SUCCEEDED.to_owned()],
        "one audit row for the outcome",
    );
}

#[tokio::test]
async fn epoch_change_is_outcome_unknown() {
    let world = seeded().await;
    let sent = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let live = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(sent));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), live);
    world.link.answer(
        host_id(),
        id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::OutcomeUnknown {
            reason: EPOCH_CHANGED.to_owned(),
        },
        "a reset journal cannot speak for a run sent under another epoch",
    );
    assert_eq!(a.journal_reason(id).await.as_deref(), Some("epoch_changed"));
    assert!(
        asked(&a).is_empty(),
        "an agent that reset its journal is not asked about the run",
    );
}

#[tokio::test]
async fn not_found_with_matching_epoch_is_not_run() {
    let (a, id) = reconciling(ActionAnswer::NotFound).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::NotRun {
            reason: NOT_RUN.to_owned(),
        },
        "a journal with no record is positive proof nothing started",
    );
    assert_eq!(
        a.trail(id).await,
        [audit::DISPATCHED.to_owned(), audit::NOT_RUN.to_owned()],
        "the run is audited as one that never happened",
    );
    assert_eq!(a.journal_reason(id).await.as_deref(), Some("not_run"));
    assert_eq!(a.runs(), 0, "nothing is sent to settle a run");
}

#[tokio::test]
async fn journal_lost_is_outcome_unknown() {
    let (a, id) = reconciling(ActionAnswer::Unknown {
        reason: UnknownReason::JournalLost,
    })
    .await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::OutcomeUnknown {
            reason: JOURNAL_LOST.to_owned(),
        },
        "a lost record is not evidence that nothing ran",
    );
    assert_eq!(a.journal_reason(id).await.as_deref(), Some("journal_lost"));
}

#[tokio::test]
async fn pre_upgrade_dispatched_row_reconciles_as_outcome_unknown() {
    // A run sent before the hub kept an epoch cannot be matched to any journal.
    let world = seeded().await;
    let live = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), None);
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), live);
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::OutcomeUnknown {
            reason: EPOCH_UNKNOWN.to_owned(),
        },
        "a run sent before the hub kept an epoch cannot be matched to a journal",
    );
    assert_eq!(a.journal_reason(id).await.as_deref(), Some("epoch_unknown"));
    assert!(asked(&a).is_empty(), "there is no epoch to ask about");
}

#[tokio::test]
async fn reconciliation_is_idempotent() {
    let (a, id) = reconciling(ActionAnswer::Finished {
        succeeded: true,
        detail: None,
    })
    .await;

    a.service.reconcile_host(host_id()).await;
    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.trail(id).await,
        [audit::DISPATCHED.to_owned(), audit::SUCCEEDED.to_owned()],
        "a second pass finds nothing left to settle",
    );
    assert_eq!(a.runs(), 0, "and never runs the skill");
}

#[tokio::test]
async fn interrupted_reconciliation_is_repeated_on_the_next_hello() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    // The first session's query fails, as a dropped link would.
    world
        .link
        .on_action_status(|_, _, _| Err(LinkError::Timeout));
    let a = world.start(ActionSettings::default(), world.store.clone());
    a.service.reconcile_host(host_id()).await;
    assert!(
        matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
        "a failed query leaves the run dispatched",
    );

    // The agent reconnects and answers this time.
    a.link.on_action_status(move |_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids
                .iter()
                .map(|id| ActionAnswerEntry {
                    execution: *id,
                    answer: ActionAnswer::Finished {
                        succeeded: true,
                        detail: None,
                    },
                })
                .collect(),
        })
    });
    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Succeeded,
        "the next authenticated hello settles the run",
    );
}

#[tokio::test]
async fn answer_for_an_unasked_id_is_ignored() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    let stranger = ActionId::new();
    world.link.set_journal_epoch(host_id(), epoch);
    a_stray_answer(&world.link, id, stranger);
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Succeeded,
        "the run the hub asked about is settled",
    );
    assert!(
        ActionRepo::get(&*a.store, stranger)
            .await
            .unwrap()
            .is_none(),
        "an answer about another id changes nothing",
    );
}

/// Makes the link answer one run as finished and invent an answer for `stranger`.
fn a_stray_answer(link: &domovoi_testkit::FakeAgentLink, id: ActionId, stranger: ActionId) {
    link.on_action_status(move |_, ids, asked_epoch| {
        let mut answers: Vec<ActionAnswerEntry> = ids
            .iter()
            .filter(|asked| **asked == id)
            .map(|asked| ActionAnswerEntry {
                execution: *asked,
                answer: ActionAnswer::Finished {
                    succeeded: true,
                    detail: None,
                },
            })
            .collect();
        answers.push(ActionAnswerEntry {
            execution: stranger,
            answer: ActionAnswer::Finished {
                succeeded: false,
                detail: Some("not asked about".to_owned()),
            },
        });
        Ok(ActionAnswers {
            epoch: asked_epoch,
            answers,
        })
    });
}

#[tokio::test]
async fn stale_answers_from_another_epoch_are_outcome_unknown() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let other = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    // The agent answers after its journal was reset: the whole batch is invalid.
    world.link.on_action_status(move |_, _, _| {
        Ok(ActionAnswers {
            epoch: other,
            answers: Vec::new(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::OutcomeUnknown {
            reason: EPOCH_CHANGED.to_owned(),
        },
        "an answer under another epoch proves nothing about the run",
    );
    assert_eq!(a.journal_reason(id).await.as_deref(), Some("epoch_changed"));
}

#[tokio::test]
async fn reconcile_query_batches_at_sixty_four_ids() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    world.link.set_journal_epoch(host_id(), epoch);
    let mut ids = Vec::new();
    for _ in 0..65 {
        let action = dispatched_for(world.incident.id(), Some(epoch));
        ids.push(action.id());
        ActionRepo::insert(&*world.store, &action).await.unwrap();
    }
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    let queries = asked(&a);
    let sizes: Vec<usize> = queries.iter().map(Vec::len).collect();
    assert_eq!(
        sizes,
        vec![64, 1],
        "one request may carry at most 64 ids, and the rest follow",
    );
    for query in &queries {
        assert!(
            query.iter().all(|id| ids.contains(id)),
            "the hub asks only about runs it still holds as dispatched",
        );
    }
    for id in ids {
        assert!(
            matches!(a.get(id).await.status(), ActionStatus::NotRun { .. }),
            "a journal with no record settles every run",
        );
    }
}

#[tokio::test]
async fn running_execution_stays_dispatched() {
    let (a, id) = running_run().await;

    assert!(
        matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
        "a run still going stays dispatched and is asked about again",
    );
}

#[tokio::test]
async fn running_is_never_redispatched() {
    let (a, _id) = running_run().await;

    assert_eq!(
        a.runs(),
        0,
        "asking the journal never sends the skill to the agent again",
    );
}

/// A run the agent says is still going, with the pass asking again on the fake clock. The
/// pass itself is aborted once it has asked twice, so the test does not wait out the whole
/// bound.
async fn running_run() -> (Actions, ActionId) {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    world.link.answer(host_id(), id, ActionAnswer::Running);
    let a = world.start(ActionSettings::default(), world.store.clone());
    let service = a.service.clone();
    let pass = tokio::spawn(async move { service.reconcile_host(host_id()).await });
    wait_for_queries(&a, 1).await;
    a.clock.advance(SignedDuration::from_secs(30));
    wait_for_queries(&a, 2).await;
    pass.abort();
    (a, id)
}

#[tokio::test]
async fn outcome_unknown_is_not_the_loop_guard() {
    // The hourly loop guard and `RAN_STATUSES` are a different rule from the unattended
    // reuse block: an unknown outcome counts as a run, and it does not stop new work.
    assert!(
        RAN_STATUSES.contains(&"outcome_unknown"),
        "a run whose outcome is unknown may have happened, so it counts as a run",
    );
    assert!(
        !RAN_STATUSES.contains(&"not_run"),
        "a run that never started is not a run",
    );

    let (a, id) = reconciling(ActionAnswer::Unknown {
        reason: UnknownReason::NoStartRecord,
    })
    .await;
    assert!(matches!(
        a.get(id).await.status(),
        ActionStatus::OutcomeUnknown { .. }
    ));

    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    a.service
        .approve(
            action.id(),
            human(UserId::new()),
            None,
            PlanBinding::Full(action.plan_hash()),
        )
        .await
        .unwrap();
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Verified)
    })
    .await;
    assert_eq!(
        a.runs(),
        1,
        "an unknown outcome does not block unrelated work through the loop guard",
    );
}
