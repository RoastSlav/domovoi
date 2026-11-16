//! Reconciling runs the hub dispatched but could no longer see the result of, when the
//! host's agent authenticates: its journal settles each run to what it recorded, to
//! `NotRun` when it has no record, or to `OutcomeUnknown` when the outcome cannot be
//! established. A run is never turned into a success or a failure without the journal
//! saying so, and nothing is written for a host with no session.
//!
//! These are the hub-side outcomes of the agent's journal crash windows; the agent side of
//! each window is tested with the journal itself, under the same names.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, ActionFilter, ActionRepo, AgentLink,
    AuditFilter, AuditLog, HostReconciler, LinkError, Page, Paged, Presence, RAN_STATUSES,
    StoreError, UnknownReason,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, HostId, IncidentId, JournalEpoch, PlanBinding, PlanHash,
    Principal, UserId,
};
use domovoi_engine::ActionSettings;
use domovoi_engine::actions::MAX_RECONCILE_PASSES;
use domovoi_engine::actions::audit;
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::T0;
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;
use tokio::sync::Semaphore;

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
const NOT_HANDED_OFF: &str = "The hub could not hand the request to the agent; it was never sent";

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
    dispatched_to(incident, host_id(), epoch)
}

/// [`dispatched_for`] for a named host.
fn dispatched_to(incident: IncidentId, host: HostId, epoch: Option<JournalEpoch>) -> ActionRequest {
    let skill = verified_restart();
    let validated = skill.validate_args(&restart_params()).unwrap();
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, host, &validated);
    let mut action = ActionRequest::propose(
        incident,
        host,
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

/// [`seed`] for a test that has a running service rather than the seeded half.
async fn seed_in(store: &InMemoryStore, action: &ActionRequest) {
    ActionRepo::insert(store, action).await.unwrap();
    AuditLog::append(store, &audit::dispatched(action, T0).into_event())
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

/// The ids each `action_status` query carried for `host`, in order.
fn asked_of(a: &Actions, host: HostId) -> Vec<Vec<ActionId>> {
    a.link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Status {
                host: asked_host,
                ids,
                ..
            } if asked_host == host => Some(ids),
            _ => None,
        })
        .collect()
}

/// Lets queued work run until `host` has been asked at least `count` times.
async fn wait_for_host_queries(a: &Actions, host: HostId, count: usize) {
    for _ in 0..5000 {
        if asked_of(a, host).len() >= count {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("reconciliation never asked {host} {count} times");
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
    let entry = AuditLog::list(
        &*a.store,
        &AuditFilter {
            subject: Some(format!("action:{id}")),
            ..AuditFilter::default()
        },
        &Page::first(10),
    )
    .await
    .unwrap()
    .items
    .into_iter()
    .find(|entry| entry.kind == audit::FAILED)
    .expect("the failure is audited");
    assert_eq!(
        entry.detail.get("reason").and_then(|value| value.as_str()),
        Some("the container is not there"),
        "the audit keeps the same reason the person sees",
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

/// What a test wants to hold: one call to the action repository, with a signal for when the
/// call is waiting and a permit that lets it continue.
struct Halt {
    reached: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

impl Halt {
    fn new() -> Self {
        Self {
            reached: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        }
    }

    /// Lets the held call continue.
    fn release(&self) {
        self.release.add_permits(1);
    }

    /// Waits until the held call is waiting. The timeout only stops a broken test from
    /// hanging; it never drives the race.
    async fn reached(&self) {
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.reached.clone().acquire_owned(),
        )
        .await
        .expect("the held call never started")
        .expect("the signal semaphore was closed");
        permit.forget();
    }
}

/// An action repository that delegates to the store but can hold one `get` for a given
/// action or one listing of dispatched rows, so a test decides what the service sees next.
struct GatedStore {
    inner: Arc<InMemoryStore>,
    hold_get: Mutex<Option<(ActionId, Arc<Halt>)>>,
    hold_dispatched: Mutex<Option<Arc<Halt>>>,
}

impl GatedStore {
    fn new(inner: Arc<InMemoryStore>) -> Self {
        Self {
            inner,
            hold_get: Mutex::new(None),
            hold_dispatched: Mutex::new(None),
        }
    }

    /// Holds the next read of `id` that finds it dispatched, which is the executor's
    /// send-time re-read rather than the writer's or the executor's earlier loads.
    fn hold_get_of_dispatched(&self, id: ActionId) -> Arc<Halt> {
        let halt = Arc::new(Halt::new());
        *self.hold_get.lock().unwrap() = Some((id, halt.clone()));
        halt
    }

    /// Holds the next listing of dispatched rows.
    fn hold_dispatched_listing(&self) -> Arc<Halt> {
        let halt = Arc::new(Halt::new());
        *self.hold_dispatched.lock().unwrap() = Some(halt.clone());
        halt
    }
}

#[async_trait]
impl ActionRepo for GatedStore {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        ActionRepo::insert(&*self.inner, action).await
    }

    async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError> {
        ActionRepo::update_from(&*self.inner, action, expected).await
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        let probe = ActionRepo::get(&*self.inner, id).await?;
        let dispatched = probe
            .as_ref()
            .is_some_and(|action| matches!(action.status(), ActionStatus::Dispatched { .. }));
        let halt = {
            let mut held = self.hold_get.lock().unwrap();
            match held.as_ref() {
                Some((held_id, _)) if *held_id == id && dispatched => {
                    held.take().map(|(_, halt)| halt)
                }
                _ => None,
            }
        };
        if let Some(halt) = halt {
            halt.reached.add_permits(1);
            let _ = halt.release.clone().acquire_owned().await;
            // Read again after the pause, so the caller sees what the test wrote meanwhile.
            return ActionRepo::get(&*self.inner, id).await;
        }
        Ok(probe)
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        let held = (filter.status.as_deref() == Some("dispatched"))
            .then(|| self.hold_dispatched.lock().unwrap().take())
            .flatten();
        if let Some(halt) = held {
            halt.reached.add_permits(1);
            let _ = halt.release.clone().acquire_owned().await;
        }
        ActionRepo::list(&*self.inner, filter, page).await
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::find_by_chat_token_hash(&*self.inner, hash).await
    }

    async fn last_run_by_skill(
        &self,
        host: domovoi_core::HostId,
    ) -> Result<BTreeMap<domovoi_core::SkillName, jiff::Timestamp>, StoreError> {
        ActionRepo::last_run_by_skill(&*self.inner, host).await
    }
}

/// Approves `action` through the service, as a person would.
async fn approve(a: &Actions, action: &ActionRequest) {
    a.service
        .approve(
            action.id(),
            human(UserId::new()),
            None,
            PlanBinding::Full(action.plan_hash()),
        )
        .await
        .unwrap();
}

/// Lets queued work run until `done` holds for the stored action, without moving time.
async fn until_dispatched(a: &Actions, id: ActionId) {
    for _ in 0..1000 {
        if matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }) {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("action {id} was never dispatched");
}

/// Lets queued work run without moving time.
async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_sent_run_is_never_mistaken_for_not_run() {
    // The write says dispatched, the request has not reached the agent, and reconciliation
    // runs: it must not list the row until the hand-off is done, or an empty journal would
    // turn a run that is about to happen into "nothing was started".
    let world = seeded().await;
    let halt = Arc::new(Semaphore::new(0));
    world.link.hold_skill_handoffs(halt.clone());
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    until_dispatched(&a, action.id()).await;
    settle().await;
    assert_eq!(a.runs(), 0, "the hand-off is held before the agent sees it");

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    settle().await;
    assert!(
        asked(&a).is_empty(),
        "reconciliation may not ask the journal while the run is still on its way",
    );

    halt.add_permits(1);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;
    let done = a
        .until(action.id(), |status| {
            !matches!(
                status,
                ActionStatus::Approved | ActionStatus::Dispatched { .. }
            )
        })
        .await;
    assert!(
        !matches!(done.status(), ActionStatus::NotRun { .. }),
        "a run the executor sent is never reported as never started: {:?}",
        done.status(),
    );
    assert_eq!(a.runs(), 1, "the skill reached the agent exactly once");
}

#[tokio::test]
async fn a_dispatch_waits_for_the_reconciliation_snapshot() {
    // Reconciliation reads the pending rows first; a dispatch starting at that moment waits
    // for the read to finish, so it cannot enter a snapshot that was taken before it.
    let world = seeded().await;
    let store = world.store.clone();
    let gated = Arc::new(GatedStore::new(store.clone()));
    let a = world
        .with_actions(gated.clone())
        .start(ActionSettings::default(), store);
    let listing = gated.hold_dispatched_listing();

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    listing.reached().await;

    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    settle().await;
    assert_eq!(
        a.get(action.id()).await.status(),
        &ActionStatus::Approved,
        "the dispatch cannot be committed while the snapshot is being read",
    );

    listing.release();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Verified)
    })
    .await;
    assert_eq!(a.runs(), 1, "the action runs once reconciliation lets go");
}

#[tokio::test]
async fn a_terminal_run_is_never_sent() {
    // The row is settled between the writer's answer and the hand-off: the executor re-reads
    // it and must not send the skill under a status that is already final.
    let world = seeded().await;
    let store = world.store.clone();
    let gated = Arc::new(GatedStore::new(store.clone()));
    let a = world
        .with_actions(gated.clone())
        .start(ActionSettings::default(), store);
    let (action, _) = a.propose().await;
    let reading = gated.hold_get_of_dispatched(action.id());

    approve(&a, &action).await;
    until_dispatched_through(&gated, action.id()).await;
    reading.reached().await;

    let mut settled = a.get(action.id()).await;
    settled
        .never_ran("a person closed it".to_owned(), T0)
        .unwrap();
    ActionRepo::update_from(&*a.store, &settled, "dispatched")
        .await
        .unwrap();
    reading.release();
    settle().await;

    assert_eq!(
        a.get(action.id()).await.status(),
        &ActionStatus::NotRun {
            reason: "a person closed it".to_owned(),
        },
        "the terminal decision stands",
    );
    assert_eq!(a.runs(), 0, "a terminal action is never sent to the agent");
}

/// Waits until the store says `id` is dispatched, without reading through the gated write
/// path the executor is about to use.
async fn until_dispatched_through(gated: &GatedStore, id: ActionId) {
    for _ in 0..1000 {
        let stored = ActionRepo::get(&*gated.inner, id).await.unwrap();
        if stored.is_some_and(|action| matches!(action.status(), ActionStatus::Dispatched { .. })) {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("action {id} was never dispatched");
}

#[tokio::test]
async fn a_lost_reply_leaves_the_run_dispatched() {
    let world = seeded().await;
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the agent disconnected".to_owned())));
    // The journal cannot answer either, so the run has no outcome to settle to.
    world
        .link
        .on_action_status(|_, _, _| Err(LinkError::Timeout));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    settle().await;

    assert!(
        matches!(
            a.get(action.id()).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "a reply lost after the hand-off says nothing about the run",
    );
    assert_eq!(
        a.trail(action.id()).await,
        [
            audit::PROPOSED.to_owned(),
            audit::APPROVED.to_owned(),
            audit::DISPATCHED.to_owned(),
        ],
        "no outcome is invented",
    );
    assert!(
        !asked(&a).is_empty(),
        "the lost result asked for reconciliation without waiting for a reconnect",
    );
}

#[tokio::test]
async fn a_hub_timeout_leaves_the_run_dispatched() {
    let world = seeded().await;
    world.link.on_skill(|_, _| Err(LinkError::Timeout));
    world
        .link
        .on_action_status(|_, _, _| Err(LinkError::Timeout));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    settle().await;

    assert!(
        matches!(
            a.get(action.id()).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "the hub's own timeout is not the agent's answer",
    );
}

#[tokio::test]
async fn an_unexpected_reply_leaves_the_run_dispatched() {
    let world = seeded().await;
    world.link.on_skill(|_, _| {
        Err(LinkError::Indeterminate(
            "the agent answered with pong".to_owned(),
        ))
    });
    world
        .link
        .on_action_status(|_, _, _| Err(LinkError::Timeout));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    settle().await;

    assert!(
        matches!(
            a.get(action.id()).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "a frame that is not a result is not a failure",
    );
}

#[tokio::test]
async fn an_agent_reported_timeout_is_a_confirmed_result() {
    let world = seeded().await;
    world
        .link
        .on_skill(|_, _| Err(LinkError::Rejected("Timeout: nginx timed out".to_owned())));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    let done = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::Failed { .. })
        })
        .await;

    assert_eq!(
        done.status(),
        &ActionStatus::Failed {
            reason: "Timeout: nginx timed out".to_owned(),
        },
        "an agent that reports a terminal failure is taken at its word",
    );
}

#[tokio::test]
async fn an_agent_refusal_is_a_confirmed_result() {
    let world = seeded().await;
    world.link.on_skill(|_, _| {
        Err(LinkError::Rejected(
            "PlanMismatch: the approved plan does not match".to_owned(),
        ))
    });
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    let done = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::Failed { .. })
        })
        .await;

    assert_eq!(
        done.status(),
        &ActionStatus::Failed {
            reason: "PlanMismatch: the approved plan does not match".to_owned(),
        },
        "a refusal with its code is a terminal result, not an unknown outcome",
    );
}

/// Asserts the facts every locally failed hand-off shares: the run is `NotRun` with the
/// hub's own reason, audited once, and never sent. No journal answer is involved.
async fn settled_as_never_handed_over(a: &Actions, id: ActionId) {
    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::NotRun {
            reason: NOT_HANDED_OFF.to_owned(),
        },
        "the hub proved no frame was queued, so the run never started",
    );
    assert_eq!(
        a.trail(id).await,
        [
            audit::PROPOSED.to_owned(),
            audit::APPROVED.to_owned(),
            audit::DISPATCHED.to_owned(),
            audit::NOT_RUN.to_owned(),
        ],
        "the failed hand-off is audited once, as a run that never happened",
    );
    assert_eq!(
        a.journal_reason(id).await.as_deref(),
        Some("hub_not_handed_off"),
        "the journal says the hub never handed the request over",
    );
    assert_eq!(a.runs(), 0, "no skill frame was ever queued");
}

/// Runs one approval whose hand-off fails with `error` and returns the service and the id.
async fn hand_off_failing(error: LinkError) -> (Actions, ActionId) {
    let world = seeded().await;
    world.link.fail_next_skill_handoff(error);
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::NotRun { .. })
    })
    .await;
    (a, action.id())
}

#[tokio::test]
async fn a_full_agent_queue_proves_the_run_was_never_sent() {
    // The agent already has every request slot taken, so `start_skill` fails before the
    // frame is queued. The hub knows more than the journal does here, and says so.
    let (a, id) = hand_off_failing(LinkError::Rejected(
        "the agent already has 256 requests in flight".to_owned(),
    ))
    .await;

    settled_as_never_handed_over(&a, id).await;
    assert!(
        asked(&a).is_empty(),
        "a locally provable non-run is not submitted to the journal",
    );
}

#[tokio::test]
async fn a_hand_off_with_no_live_session_proves_the_run_was_never_sent() {
    // The agent disappears between the dispatch commit and the hand-off. No session means
    // no queue, so the request cannot have reached the agent.
    let world = seeded().await;
    let store = world.store.clone();
    let gated = Arc::new(GatedStore::new(store.clone()));
    let a = world
        .with_actions(gated.clone())
        .start(ActionSettings::default(), store);
    let (action, _) = a.propose().await;
    let reading = gated.hold_get_of_dispatched(action.id());
    approve(&a, &action).await;
    until_dispatched_through(&gated, action.id()).await;
    reading.reached().await;

    a.link.set_presence(
        host_id(),
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
    reading.release();
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::NotRun { .. })
    })
    .await;

    settled_as_never_handed_over(&a, action.id()).await;
    assert!(asked(&a).is_empty(), "there is no session to ask");
}

#[tokio::test]
async fn a_hand_off_that_timed_out_before_sending_proves_the_run_was_never_sent() {
    // The deadline is checked before the frame is dispatched, so a timeout there is not the
    // same as a request that was sent and never answered.
    let (a, id) = hand_off_failing(LinkError::Timeout).await;

    settled_as_never_handed_over(&a, id).await;
}

#[tokio::test]
async fn an_unencodable_hand_off_proves_the_run_was_never_sent() {
    // Encoding happens before the frame enters the outgoing queue, so it is another local
    // proof that nothing could have run.
    let (a, id) = hand_off_failing(LinkError::Transport(
        "the request could not be encoded".to_owned(),
    ))
    .await;

    settled_as_never_handed_over(&a, id).await;
}

#[tokio::test]
async fn a_failed_hand_off_still_counts_as_a_dispatch_attempt_after_a_restart() {
    // The loop guard counts committed dispatch attempts, and recovery rebuilds them from the
    // same `action.dispatched` audit rows, so a restart neither forgets nor invents attempts
    // whose hand-off never reached the agent.
    let world = seeded().await;
    let settings = ActionSettings {
        loop_guard_per_hour: 2,
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());
    for _ in 0..2 {
        world.link.fail_next_skill_handoff(LinkError::Rejected(
            "the agent already has 256 requests in flight".to_owned(),
        ));
        let (action, _) = a.propose().await;
        approve(&a, &action).await;
        a.until(action.id(), |status| {
            matches!(status, ActionStatus::NotRun { .. })
        })
        .await;
        settled_as_never_handed_over(&a, action.id()).await;
    }

    // The next approval cannot even try: the hour's two attempts are used up.
    let (blocked, _) = a.propose().await;
    approve(&a, &blocked).await;
    let blocked = a
        .until(blocked.id(), |status| {
            matches!(status, ActionStatus::Rejected { .. })
        })
        .await;
    assert!(
        matches!(
            blocked.status(),
            ActionStatus::Rejected { reason } if reason.starts_with("loop guard:")
        ),
        "the two committed attempts fill the hour: {:?}",
        blocked.status(),
    );
    drop(a);

    // A restarted hub reads the same audit rows back, so the guard is just as full.
    let restarted = world.start(settings, world.store.clone());
    let (again, _) = restarted.propose().await;
    approve(&restarted, &again).await;
    let again = restarted
        .until(again.id(), |status| {
            matches!(status, ActionStatus::Rejected { .. })
        })
        .await;
    assert!(
        matches!(
            again.status(),
            ActionStatus::Rejected { reason } if reason.starts_with("loop guard:")
        ),
        "recovery counts the same attempts the running hub did: {:?}",
        again.status(),
    );
}
#[tokio::test]
async fn an_old_epoch_request_is_never_sent() {
    // The journal changed between the dispatch and the hand-off. Sending to the replacement
    // would attach the run to a journal it was never recorded under.
    let world = seeded().await;
    let store = world.store.clone();
    let link = world.link.clone();
    let gated = Arc::new(GatedStore::new(store.clone()));
    let a = world
        .with_actions(gated.clone())
        .start(ActionSettings::default(), store);
    let (action, _) = a.propose().await;
    let reading = gated.hold_get_of_dispatched(action.id());
    approve(&a, &action).await;
    until_dispatched_through(&gated, action.id()).await;
    reading.reached().await;

    link.set_journal_epoch(host_id(), JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
    reading.release();
    settle().await;

    assert!(
        matches!(
            a.get(action.id()).await.status(),
            ActionStatus::NotRun { .. }
        ),
        "the hub refused to send under the old epoch, so the run never started: {:?}",
        a.get(action.id()).await.status(),
    );
    settled_as_never_handed_over(&a, action.id()).await;
    assert_eq!(
        a.runs(),
        0,
        "the replacement session never received the old epoch's run",
    );
}

#[tokio::test]
async fn a_reconciliation_pass_stops_when_the_journal_changes_while_listing() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    let store = world.store.clone();
    let link = world.link.clone();
    link.set_journal_epoch(host_id(), epoch);
    let gated = Arc::new(GatedStore::new(store.clone()));
    let a = world
        .with_actions(gated.clone())
        .start(ActionSettings::default(), store);
    let listing = gated.hold_dispatched_listing();

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    listing.reached().await;
    // A run dispatched under the replacement session while the old pass is reading.
    let replacement = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let newer = dispatched_for(a.incident.id(), Some(replacement));
    ActionRepo::insert(&*a.store, &newer).await.unwrap();
    link.set_journal_epoch(host_id(), replacement);
    listing.release();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;

    assert!(
        asked(&a).is_empty(),
        "the pass does not ask a replacement session about the old epoch's runs",
    );
    assert!(
        matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
        "the old epoch's run is left for the replacement's own pass",
    );
    assert!(
        matches!(
            a.get(newer.id()).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "a run from the new session is not classified against the old epoch",
    );
}

/// A dispatched run on `host`, settled by whatever `answers` the agent gives, so a test can
/// try the malformed shapes one at a time.
async fn duplicate_world(
    answer: impl Fn(Vec<ActionId>, JournalEpoch) -> ActionAnswers + Send + Sync + 'static,
) -> (Actions, ActionId) {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    world
        .link
        .on_action_status(move |_, ids, asked| Ok(answer(ids.to_vec(), asked)));
    let a = world.start(ActionSettings::default(), world.store.clone());
    a.service.reconcile_host(host_id()).await;
    (a, id)
}

/// A completed run for one id.
fn finished(id: ActionId) -> ActionAnswerEntry {
    ActionAnswerEntry {
        execution: id,
        answer: ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    }
}

/// A lost record for one id.
fn journal_lost(id: ActionId) -> ActionAnswerEntry {
    ActionAnswerEntry {
        execution: id,
        answer: ActionAnswer::Unknown {
            reason: UnknownReason::JournalLost,
        },
    }
}

#[tokio::test]
async fn contradictory_duplicate_answers_settle_nothing() {
    for (first, second) in [
        (
            finished as fn(ActionId) -> ActionAnswerEntry,
            journal_lost as fn(ActionId) -> ActionAnswerEntry,
        ),
        (journal_lost, finished),
        (finished, finished),
    ] {
        let (a, id) = duplicate_world(move |ids, epoch| ActionAnswers {
            epoch,
            answers: ids
                .into_iter()
                .flat_map(|id| [first(id), second(id)])
                .collect(),
        })
        .await;

        assert!(
            matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
            "a duplicated answer is malformed and must not settle the run: {:?}",
            a.get(id).await.status(),
        );
        assert_eq!(
            a.trail(id).await,
            [audit::DISPATCHED.to_owned()],
            "no outcome is recorded for a malformed answer",
        );
    }
}

#[tokio::test]
async fn a_running_duplicate_leaves_the_run_dispatched() {
    let (a, id) = duplicate_world(|ids, epoch| ActionAnswers {
        epoch,
        answers: ids
            .into_iter()
            .flat_map(|id| {
                [
                    ActionAnswerEntry {
                        execution: id,
                        answer: ActionAnswer::Running,
                    },
                    finished(id),
                ]
            })
            .collect(),
    })
    .await;

    assert!(
        matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
        "a running answer followed by a terminal one is a malformed batch",
    );
    assert_eq!(
        a.trail(id).await,
        [audit::DISPATCHED.to_owned()],
        "the duplicate did not settle the run",
    );
}

#[tokio::test]
async fn a_duplicate_does_not_discard_a_valid_neighbour() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let duplicated = dispatched_for(world.incident.id(), Some(epoch));
    let valid = dispatched_for(world.incident.id(), Some(epoch));
    let (duplicated_id, valid_id) = (duplicated.id(), valid.id());
    seed(&world, &duplicated).await;
    seed(&world, &valid).await;
    world.link.set_journal_epoch(host_id(), epoch);
    world.link.on_action_status(move |_, ids, asked| {
        let mut answers = Vec::new();
        for id in ids {
            if *id == duplicated_id {
                answers.push(finished(*id));
                answers.push(finished(*id));
            } else {
                answers.push(finished(*id));
            }
        }
        Ok(ActionAnswers {
            epoch: asked,
            answers,
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    assert!(
        matches!(
            a.get(duplicated_id).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "the malformed id stays dispatched",
    );
    assert_eq!(
        a.get(valid_id).await.status(),
        &ActionStatus::Succeeded,
        "a valid unique answer in the same batch is still applied",
    );
}
#[tokio::test]
async fn an_answer_from_the_matching_session_is_committed_after_a_replacement() {
    // The answer is received while the session still matches, so it is legitimate even
    // though the agent reconnects before the pass finishes.
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    let link = world.link.clone();
    world.link.on_action_status(move |_, ids, asked| {
        // The replacement happens after the answer was produced under `asked`.
        link.set_journal_epoch(host_id(), JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        Ok(ActionAnswers {
            epoch: asked,
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
    let a = world.start(ActionSettings::default(), world.store.clone());

    a.service.reconcile_host(host_id()).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Succeeded,
        "evidence received from the matching session is not discarded",
    );
}

#[tokio::test]
async fn a_lost_reply_is_recovered_from_the_journal() {
    // The whole path: the agent ran the skill and recorded it, the reply never arrived, and
    // the session stays connected. The executor asks for a pass itself, and the journal
    // settles the run without waiting for a reconnect.
    let world = seeded().await;
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    a.link.answer(
        host_id(),
        action.id(),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    approve(&a, &action).await;
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Succeeded)
    })
    .await;

    assert_eq!(
        a.get(action.id()).await.status(),
        &ActionStatus::Succeeded,
        "the journal's recorded result settles the run",
    );
    assert_eq!(
        a.trail(action.id()).await,
        [
            audit::PROPOSED.to_owned(),
            audit::APPROVED.to_owned(),
            audit::DISPATCHED.to_owned(),
            audit::SUCCEEDED.to_owned(),
        ],
        "exactly one terminal audit entry",
    );
}

#[tokio::test]
async fn a_lost_reply_about_a_failure_keeps_the_reason() {
    let world = seeded().await;
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    a.link.answer(
        host_id(),
        action.id(),
        ActionAnswer::Finished {
            succeeded: false,
            detail: Some("exited_non_zero: the service did not stop".to_owned()),
        },
    );
    approve(&a, &action).await;
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Failed { .. })
    })
    .await;

    assert_eq!(
        a.get(action.id()).await.status(),
        &ActionStatus::Failed {
            reason: "exited_non_zero: the service did not stop".to_owned(),
        },
        "a recovered failure keeps its code and message",
    );
}

#[tokio::test]
async fn many_lost_replies_do_not_multiply_the_query_workload() {
    // Five lost replies each ask for a reconciliation. A trigger that arrives while a pass
    // runs is covered by that pass instead of starting a second one, so the journal is asked
    // at most once per run, never on a loop of its own.
    let world = seeded().await;
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let settings = ActionSettings {
        loop_guard_per_hour: 10,
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());
    let mut actions = Vec::new();
    for _ in 0..5 {
        let (action, _) = a.propose().await;
        a.link.answer(
            host_id(),
            action.id(),
            ActionAnswer::Finished {
                succeeded: true,
                detail: None,
            },
        );
        actions.push(action);
    }
    for action in &actions {
        approve(&a, action).await;
    }
    for action in &actions {
        a.until(action.id(), |status| {
            matches!(status, ActionStatus::Succeeded)
        })
        .await;
    }

    assert!(
        asked(&a).len() <= actions.len(),
        "each trigger is covered by at most one query, not a poll loop: {}",
        asked(&a).len(),
    );
    for action in actions {
        assert_eq!(
            a.trail(action.id()).await,
            [
                audit::PROPOSED.to_owned(),
                audit::APPROVED.to_owned(),
                audit::DISPATCHED.to_owned(),
                audit::SUCCEEDED.to_owned(),
            ],
            "each run is settled exactly once",
        );
    }
}

#[tokio::test]
async fn a_missing_answer_is_asked_again() {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    let calls = Arc::new(std::sync::Mutex::new(0u32));
    let seen = calls.clone();
    world.link.on_action_status(move |_, ids, asked| {
        let mut count = seen.lock().unwrap();
        *count += 1;
        if *count == 1 {
            // The agent says nothing about the id this time.
            return Ok(ActionAnswers {
                epoch: asked,
                answers: Vec::new(),
            });
        }
        Ok(ActionAnswers {
            epoch: asked,
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
    let a = world.start(ActionSettings::default(), world.store.clone());

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    wait_for_queries(&a, 1).await;
    a.clock.advance(SignedDuration::from_secs(30));
    wait_for_queries(&a, 2).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Succeeded,
        "a run the agent left out of its answer is asked about again",
    );
}

/// A unit of work that holds the first commit until a test releases it, so the writer queue
/// can fill while a pass hands answers over.
struct StallingUow {
    store: Arc<InMemoryStore>,
    release: Arc<Semaphore>,
    stalled: Arc<Semaphore>,
    stalled_once: std::sync::atomic::AtomicBool,
}

impl StallingUow {
    fn new(store: Arc<InMemoryStore>) -> Self {
        Self {
            store,
            release: Arc::new(Semaphore::new(0)),
            stalled: Arc::new(Semaphore::new(0)),
            stalled_once: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl domovoi_core::ports::UnitOfWork for StallingUow {
    async fn commit(
        &self,
        tx: domovoi_core::ports::Transaction,
    ) -> Result<domovoi_core::ports::CommitReceipt, domovoi_core::ports::CommitError> {
        use std::sync::atomic::Ordering;
        if !self.stalled_once.swap(true, Ordering::SeqCst) {
            self.stalled.add_permits(1);
            let _ = self.release.clone().acquire_owned().await;
        }
        domovoi_core::ports::UnitOfWork::commit(&*self.store, tx).await
    }
}

#[tokio::test]
async fn a_full_writer_queue_defers_answers_without_losing_them() {
    // More answers than the writer queue holds. The overflow goes to the one deferred worker,
    // and aborting the pass after it handed them over must not lose any of them.
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    world.link.set_journal_epoch(host_id(), epoch);
    let mut ids = Vec::new();
    for _ in 0..70 {
        let action = dispatched_for(world.incident.id(), Some(epoch));
        ids.push(action.id());
        ActionRepo::insert(&*world.store, &action).await.unwrap();
    }
    world.link.on_action_status(|_, ids, asked| {
        Ok(ActionAnswers {
            epoch: asked,
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
    let uow = Arc::new(StallingUow::new(world.store.clone()));
    let a = world.start(ActionSettings::default(), uow.clone());

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    // The writer is stalled on its first commit; let the pass queue everything it has.
    uow.stalled
        .clone()
        .acquire_owned()
        .await
        .expect("stalled")
        .forget();
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }
    pass.abort();
    uow.release.add_permits(1);

    a.until(ids[0], |status| matches!(status, ActionStatus::Succeeded))
        .await;
    for id in ids {
        assert!(
            matches!(a.get(id).await.status(), ActionStatus::Succeeded),
            "every handed-over answer is committed, including the queue overflow",
        );
    }
}

#[tokio::test]
async fn a_legacy_lost_reply_becomes_outcome_unknown() {
    // The run may have happened, and the agent cannot account for it: the only truthful
    // outcome is one that says so.
    let world = seeded().await;
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(move |_, ids, asked| {
        Ok(ActionAnswers {
            epoch: asked,
            answers: ids
                .iter()
                .map(|id| ActionAnswerEntry {
                    execution: *id,
                    answer: ActionAnswer::Unknown {
                        reason: UnknownReason::JournalLost,
                    },
                })
                .collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    let done = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::OutcomeUnknown { .. })
        })
        .await;

    assert!(
        matches!(done.status(), ActionStatus::OutcomeUnknown { .. }),
        "a legacy run the agent cannot account for is never reported as never run: {:?}",
        done.status(),
    );
    assert_eq!(
        a.journal_reason(action.id()).await.as_deref(),
        Some("journal_lost")
    );
}

/// Adds a connected agent with a journal and the same skills to `host`.
fn connect_host(world: &Seeded, host: HostId, epoch: JournalEpoch) {
    world.link.set_presence(
        host,
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    world.link.set_journal_epoch(host, epoch);
    world.link.set_skills(host, vec![verified_restart()]);
}

/// An approved `restart_container` on `host`, ready for the executor's catch-up.
fn approved_for_host(incident: IncidentId, host: HostId) -> ActionRequest {
    let mut action = proposed_for_host(incident, host);
    action.approve(human(UserId::new()), T0).unwrap();
    action
}

/// An unapproved `restart_container` on `host`.
fn proposed_for_host(incident: IncidentId, host: HostId) -> ActionRequest {
    let skill = verified_restart();
    let validated = skill.validate_args(&restart_params()).unwrap();
    ActionRequest::propose(
        incident,
        host,
        &skill,
        &validated,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    )
}

/// Waits until the executor has handed `id`'s skill to the agent exactly once.
async fn wait_for_one_run(a: &Actions) {
    for _ in 0..2000 {
        if a.runs() >= 1 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the skill was never handed over");
}

#[tokio::test]
async fn a_trigger_that_arrives_during_a_pass_is_not_lost() {
    // The first pass takes its snapshot, then a run is dispatched and loses its reply. The
    // running pass cannot see that run, so the trigger must survive its gate.
    let world = seeded().await;
    let epoch = world
        .link
        .journal_epoch(host_id())
        .expect("the agent has a journal");
    let earlier = dispatched_for(world.incident.id(), Some(epoch));
    let earlier_id = earlier.id();
    seed(&world, &earlier).await;
    world.link.answer(
        host_id(),
        earlier_id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let gate = Arc::new(Semaphore::new(0));
    world.link.hold_status_queries(host_id(), gate.clone());
    let a = world.start(ActionSettings::default(), world.store.clone());

    let first = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    wait_for_host_queries(&a, host_id(), 1).await;
    assert!(
        matches!(
            a.get(earlier_id).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "the first pass is held inside its query, after its snapshot",
    );

    let (later, _) = a.propose().await;
    a.link.answer(
        host_id(),
        later.id(),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    approve(&a, &later).await;
    until_dispatched(&a, later.id()).await;
    wait_for_one_run(&a).await;
    a.settle().await;

    gate.add_permits(2);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), first).await;
    a.until(later.id(), |status| {
        matches!(status, ActionStatus::Succeeded)
    })
    .await;

    assert_eq!(
        a.get(earlier_id).await.status(),
        &ActionStatus::Succeeded,
        "the first pass settled the run it had snapshotted",
    );
    let queries = asked_of(&a, host_id());
    assert_eq!(
        queries.len(),
        2,
        "the trigger produced exactly one follow-up pass",
    );
    assert!(
        queries[1].contains(&later.id()),
        "the follow-up pass asks about the run dispatched after the snapshot",
    );
}

#[tokio::test]
async fn a_burst_of_triggers_during_one_pass_runs_one_follow_up_pass() {
    let world = seeded().await;
    let epoch = world
        .link
        .journal_epoch(host_id())
        .expect("the agent has a journal");
    let earlier = dispatched_for(world.incident.id(), Some(epoch));
    let earlier_id = earlier.id();
    seed(&world, &earlier).await;
    world.link.answer(
        host_id(),
        earlier_id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let gate = Arc::new(Semaphore::new(0));
    world.link.hold_status_queries(host_id(), gate.clone());
    let settings = ActionSettings {
        loop_guard_per_hour: 10,
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());

    let first = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    wait_for_host_queries(&a, host_id(), 1).await;

    let mut later = Vec::new();
    for expected_runs in 1..=3 {
        let (action, _) = a.propose().await;
        a.link.answer(
            host_id(),
            action.id(),
            ActionAnswer::Finished {
                succeeded: true,
                detail: None,
            },
        );
        approve(&a, &action).await;
        until_dispatched(&a, action.id()).await;
        for _ in 0..2000 {
            if a.runs() >= expected_runs {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(a.runs(), expected_runs, "the run was handed over once");
        a.settle().await;
        later.push(action.id());
    }

    gate.add_permits(2);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), first).await;
    for id in &later {
        a.until(*id, |status| matches!(status, ActionStatus::Succeeded))
            .await;
        assert_eq!(
            a.trail(*id).await,
            [
                audit::PROPOSED.to_owned(),
                audit::APPROVED.to_owned(),
                audit::DISPATCHED.to_owned(),
                audit::SUCCEEDED.to_owned(),
            ],
            "the follow-up pass settles each run once",
        );
    }

    let queries = asked_of(&a, host_id());
    assert_eq!(
        queries.len(),
        2,
        "three triggers during one pass produce one follow-up pass, not three",
    );
    assert_eq!(
        queries[1].len(),
        3,
        "the follow-up pass asks about all three runs in one query",
    );
}

#[tokio::test]
async fn a_pass_under_a_replaced_session_never_uses_the_new_epoch() {
    let world = seeded().await;
    let old = world
        .link
        .journal_epoch(host_id())
        .expect("the agent has a journal");
    let earlier = dispatched_for(world.incident.id(), Some(old));
    let earlier_id = earlier.id();
    seed(&world, &earlier).await;
    world.link.answer(
        host_id(),
        earlier_id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    let gate = Arc::new(Semaphore::new(0));
    world.link.hold_status_queries(host_id(), gate.clone());
    let a = world.start(ActionSettings::default(), world.store.clone());

    let first = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    wait_for_host_queries(&a, host_id(), 1).await;
    assert_eq!(
        asked_of(&a, host_id())[0],
        vec![earlier_id],
        "the first query belongs to the old session's pass",
    );

    // The agent reconnects with a new journal while the old query is held, and a run is
    // dispatched under it.
    let new = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    a.link.set_journal_epoch(host_id(), new);
    let newer = dispatched_for(a.incident.id(), Some(new));
    let newer_id = newer.id();
    seed_in(&a.store, &newer).await;
    a.link.answer(
        host_id(),
        newer_id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    // The replacement's own session-tied pass waits for the gate, then reads under `new`.
    let second = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });

    gate.add_permits(2);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), first).await;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), second).await;

    assert_eq!(
        a.get(earlier_id).await.status(),
        &ActionStatus::Succeeded,
        "an answer received under the old session is still committed",
    );
    assert_eq!(
        a.get(newer_id).await.status(),
        &ActionStatus::Succeeded,
        "the replacement's pass settles the run sent under its journal",
    );
    let queries = a
        .link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Status {
                host,
                ids,
                expected_epoch,
                ..
            } if host == host_id() => Some((ids, expected_epoch)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (ids, epoch) in queries {
        assert_eq!(
            epoch,
            if ids.contains(&newer_id) { new } else { old },
            "a run is only ever asked about under the journal it was dispatched to: {ids:?}",
        );
    }
}

#[tokio::test]
async fn a_cancelled_pass_releases_the_gate_and_keeps_the_trigger() {
    let world = seeded().await;
    let epoch = world
        .link
        .journal_epoch(host_id())
        .expect("the agent has a journal");
    let earlier = dispatched_for(world.incident.id(), Some(epoch));
    let earlier_id = earlier.id();
    seed(&world, &earlier).await;
    world.link.answer(
        host_id(),
        earlier_id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let gate = Arc::new(Semaphore::new(0));
    world.link.hold_status_queries(host_id(), gate.clone());
    let a = world.start(ActionSettings::default(), world.store.clone());

    let first = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    wait_for_host_queries(&a, host_id(), 1).await;

    let (later, _) = a.propose().await;
    a.link.answer(
        host_id(),
        later.id(),
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    approve(&a, &later).await;
    until_dispatched(&a, later.id()).await;
    wait_for_one_run(&a).await;
    a.settle().await;

    first.abort();
    let _ = first.await;
    assert!(
        matches!(
            a.get(earlier_id).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "an aborted pass settles nothing",
    );

    gate.add_permits(2);
    a.until(later.id(), |status| {
        matches!(status, ActionStatus::Succeeded)
    })
    .await;
    assert_eq!(
        a.get(earlier_id).await.status(),
        &ActionStatus::Succeeded,
        "the follow-up pass released by the cancelled one settles the rest",
    );
    assert_eq!(
        asked_of(&a, host_id()).len(),
        2,
        "the trigger was not stranded by the cancellation",
    );
}

#[tokio::test]
async fn a_slow_host_does_not_hold_up_another_host() {
    let world = seeded().await;
    let slow = host_id();
    let fast = HostId::new();
    connect_host(&world, fast, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
    let slow_run = approved_for_host(world.incident.id(), slow);
    let fast_run = approved_for_host(world.incident.id(), fast);
    ActionRepo::insert(&*world.store, &slow_run).await.unwrap();
    ActionRepo::insert(&*world.store, &fast_run).await.unwrap();
    let gate = Arc::new(Semaphore::new(0));
    world.link.hold_status_queries(slow, gate.clone());
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    wait_for_host_queries(&a, slow, 1).await;
    let done = a
        .until(fast_run.id(), |status| {
            matches!(status, ActionStatus::Succeeded)
        })
        .await;
    assert_eq!(
        done.status(),
        &ActionStatus::Succeeded,
        "the other host's run settles while the slow host's query is held",
    );
    assert!(
        matches!(
            a.get(slow_run.id()).await.status(),
            ActionStatus::Dispatched { .. }
        ),
        "the slow host's pass is still active",
    );
    assert_eq!(asked_of(&a, slow).len(), 1);

    gate.add_permits(1);
    a.until(slow_run.id(), |status| {
        matches!(status, ActionStatus::Succeeded)
    })
    .await;
}

#[tokio::test]
async fn a_full_pool_delays_passes_without_losing_them() {
    let world = seeded().await;
    let mut hosts = Vec::new();
    let mut runs = Vec::new();
    for i in 0..6 {
        let host = if i == 0 { host_id() } else { HostId::new() };
        if i > 0 {
            connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        }
        let run = approved_for_host(world.incident.id(), host);
        ActionRepo::insert(&*world.store, &run).await.unwrap();
        hosts.push(host);
        runs.push(run);
    }
    let gate = Arc::new(Semaphore::new(0));
    for host in &hosts {
        world.link.hold_status_queries(*host, gate.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let settings = ActionSettings {
        loop_guard_per_hour: 10,
        ..ActionSettings::default()
    };
    let a = world.start(settings, world.store.clone());

    for _ in 0..5000 {
        if asked(&a).len() >= MAX_RECONCILE_PASSES {
            break;
        }
        tokio::task::yield_now().await;
    }
    for _ in 0..500 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        asked(&a).len(),
        MAX_RECONCILE_PASSES,
        "the pool bound holds even with more hosts asking",
    );

    // A host still waiting for a slot gets a second lost reply. Both triggers are waiting,
    // so they must collapse into the one pass it is about to get.
    let waiting = *hosts
        .iter()
        .find(|host| asked_of(&a, **host).is_empty())
        .expect("a host is waiting for a slot");
    let before = a.runs();
    let extra = a
        .service
        .propose(proposed_for_host(a.incident.id(), waiting))
        .await
        .unwrap();
    approve(&a, &extra).await;
    for _ in 0..2000 {
        if a.runs() > before {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(a.runs() > before, "the second run was handed over");
    a.settle().await;
    assert!(
        asked_of(&a, waiting).is_empty(),
        "the waiting host still has no pass while the pool is full",
    );

    gate.add_permits(hosts.len());
    runs.push(extra);
    for run in &runs {
        a.until(run.id(), |status| matches!(status, ActionStatus::Succeeded))
            .await;
    }
    let queries = asked_of(&a, waiting);
    assert_eq!(
        queries.len(),
        1,
        "two triggers for the waiting host produce one pass, not two",
    );
    assert_eq!(
        queries[0].len(),
        2,
        "the one pass asks about both of the host's runs",
    );
    assert_eq!(
        asked(&a).len(),
        hosts.len(),
        "each host gets one pass once a slot frees, and the extra run shares one",
    );
}

#[tokio::test]
async fn shutdown_joins_passes_held_in_a_query() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    let other = HostId::new();
    connect_host(&world, other, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
    hosts.push(other);
    let mut runs = Vec::new();
    for host in &hosts {
        let run = approved_for_host(world.incident.id(), *host);
        ActionRepo::insert(&*world.store, &run).await.unwrap();
        runs.push(run);
    }
    let gate = Arc::new(Semaphore::new(0));
    for host in &hosts {
        world.link.hold_status_queries(*host, gate.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    let mut a = world.start(ActionSettings::default(), world.store.clone());
    for host in &hosts {
        wait_for_host_queries(&a, *host, 1).await;
    }

    a.shutdown.cancel();
    let joined = tokio::time::timeout(std::time::Duration::from_secs(5), &mut a.action_task).await;
    assert!(
        joined.is_ok(),
        "the service joined its held reconciliation passes on shutdown",
    );
    let _ = runs;
}

/// An action repository whose paged listing fails on chosen 1-based calls and records the
/// cursor of every call, so a test can prove a retry starts the whole listing over.
struct FlakyListing {
    inner: Arc<InMemoryStore>,
    calls: Mutex<u32>,
    cursors: Mutex<Vec<Option<String>>>,
    fail_on: Vec<u32>,
    announce_second_page_on: Option<u32>,
    failed: Arc<Semaphore>,
}

impl FlakyListing {
    fn new(
        inner: Arc<InMemoryStore>,
        fail_on: Vec<u32>,
        announce_second_page_on: Option<u32>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            calls: Mutex::new(0),
            cursors: Mutex::new(Vec::new()),
            fail_on,
            announce_second_page_on,
            failed: Arc::new(Semaphore::new(0)),
        })
    }

    fn cursors(&self) -> Vec<Option<String>> {
        self.cursors.lock().unwrap().clone()
    }

    /// Waits for the next scripted failure to happen.
    async fn failure(&self) {
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.failed.clone().acquire_owned(),
        )
        .await
        .expect("the listing never failed")
        .expect("the signal semaphore was closed");
        permit.forget();
    }
}

#[async_trait]
impl ActionRepo for FlakyListing {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        ActionRepo::insert(&*self.inner, action).await
    }

    async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError> {
        ActionRepo::update_from(&*self.inner, action, expected).await
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::get(&*self.inner, id).await
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        // Only reconciliation's listing of dispatched rows is scripted; the executor's
        // start-up scan reads approved rows through the same store and must not use up a
        // scripted call.
        if filter.status.as_deref() != Some("dispatched") {
            return ActionRepo::list(&*self.inner, filter, page).await;
        }
        let call = {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            *calls
        };
        self.cursors.lock().unwrap().push(page.cursor.clone());
        if self.fail_on.contains(&call) {
            self.failed.add_permits(1);
            return Err(StoreError::Backend("the store hiccuped".to_owned()));
        }
        let mut got = ActionRepo::list(&*self.inner, filter, page).await?;
        if self.announce_second_page_on == Some(call) {
            got.next_cursor = Some("page-2".to_owned());
        }
        Ok(got)
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::find_by_chat_token_hash(&*self.inner, hash).await
    }

    async fn last_run_by_skill(
        &self,
        host: HostId,
    ) -> Result<BTreeMap<domovoi_core::SkillName, jiff::Timestamp>, StoreError> {
        ActionRepo::last_run_by_skill(&*self.inner, host).await
    }
}

/// A dispatched run under `epoch` whose journal answers `answer`, for a listing test.
async fn listing_world(answer: ActionAnswer) -> (Seeded, Arc<InMemoryStore>, ActionId) {
    let world = seeded().await;
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::now_v7());
    let action = dispatched_for(world.incident.id(), Some(epoch));
    let id = action.id();
    seed(&world, &action).await;
    world.link.set_journal_epoch(host_id(), epoch);
    world.link.answer(host_id(), id, answer);
    let store = world.store.clone();
    (world, store, id)
}

#[tokio::test]
async fn a_transient_listing_failure_recovers_without_a_reconnect() {
    let (world, store, id) = listing_world(ActionAnswer::Finished {
        succeeded: true,
        detail: None,
    })
    .await;
    let flaky = FlakyListing::new(store.clone(), vec![1], None);
    let a = world
        .with_actions(flaky.clone())
        .start(ActionSettings::default(), store);

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    flaky.failure().await;
    a.clock.advance(SignedDuration::from_millis(500));
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;

    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Succeeded,
        "the same session retries the listing and settles the run",
    );
    assert_eq!(
        flaky.cursors(),
        vec![None, None],
        "the retry starts from the first page",
    );
    assert_eq!(asked(&a).len(), 1, "the run is asked about exactly once");
}

#[tokio::test]
async fn a_failed_later_page_restarts_the_listing_from_the_first_page() {
    let (world, store, id) = listing_world(ActionAnswer::Finished {
        succeeded: true,
        detail: None,
    })
    .await;
    let other = dispatched_for(
        world.incident.id(),
        Some(
            world
                .link
                .journal_epoch(host_id())
                .expect("the agent has a journal"),
        ),
    );
    let other_id = other.id();
    seed(&world, &other).await;
    world.link.answer(
        host_id(),
        other_id,
        ActionAnswer::Finished {
            succeeded: true,
            detail: None,
        },
    );
    let flaky = FlakyListing::new(store.clone(), vec![2], Some(1));
    let a = world
        .with_actions(flaky.clone())
        .start(ActionSettings::default(), store);

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    flaky.failure().await;
    a.clock.advance(SignedDuration::from_millis(500));
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;

    assert_eq!(
        flaky.cursors(),
        vec![None, Some("page-2".to_owned()), None],
        "a failed page discards the pages already read and starts over",
    );
    assert_eq!(a.get(id).await.status(), &ActionStatus::Succeeded);
    assert_eq!(a.get(other_id).await.status(), &ActionStatus::Succeeded);
    assert_eq!(
        asked(&a).len(),
        1,
        "the complete listing is queried once, not once per attempt",
    );
}

#[tokio::test]
async fn a_listing_that_never_finishes_settles_nothing() {
    let (world, store, id) = listing_world(ActionAnswer::Finished {
        succeeded: true,
        detail: None,
    })
    .await;
    let flaky = FlakyListing::new(store.clone(), vec![1, 2, 3], None);
    let a = world
        .with_actions(flaky.clone())
        .start(ActionSettings::default(), store);

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    for _ in 0..3 {
        flaky.failure().await;
        a.clock.advance(SignedDuration::from_millis(500));
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;

    assert_eq!(
        flaky.cursors(),
        vec![None, None, None],
        "each attempt starts from the first page",
    );
    assert!(
        asked(&a).is_empty(),
        "a listing that never finished is never queried",
    );
    assert!(
        matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
        "the run stays dispatched for the next session",
    );
}

#[tokio::test]
async fn a_listing_retry_never_classifies_against_the_replacement_epoch() {
    let (world, store, id) = listing_world(ActionAnswer::Finished {
        succeeded: true,
        detail: None,
    })
    .await;
    let flaky = FlakyListing::new(store.clone(), vec![1], None);
    let a = world
        .with_actions(flaky.clone())
        .start(ActionSettings::default(), store);

    let pass = tokio::spawn({
        let service = a.service.clone();
        async move { service.reconcile_host(host_id()).await }
    });
    flaky.failure().await;
    // The agent reconnects with a new journal before the retry reads the rows.
    a.link
        .set_journal_epoch(host_id(), JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
    a.clock.advance(SignedDuration::from_millis(500));
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;

    assert!(
        asked(&a).is_empty(),
        "the old pass never asks the replacement about the old journal's run",
    );
    assert!(
        matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
        "the replacement's own pass will settle the run",
    );
}

#[tokio::test]
async fn hello_passes_take_the_same_bounded_pool() {
    // Six sessions ask for a pass at once. The pool bounds them just like the queued ones,
    // so no more than its size reads a journal at the same time.
    let world = seeded().await;
    let mut hosts = Vec::new();
    let mut actions = Vec::new();
    for i in 0..6 {
        let host = if i == 0 { host_id() } else { HostId::new() };
        if i > 0 {
            connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        }
        let epoch = world.link.journal_epoch(host).expect("a journal");
        let action = dispatched_to(world.incident.id(), host, Some(epoch));
        seed_in(&world.store, &action).await;
        hosts.push(host);
        actions.push(action);
    }
    let gate = Arc::new(Semaphore::new(0));
    for host in &hosts {
        world.link.hold_status_queries(*host, gate.clone());
    }
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    let mut passes = Vec::new();
    for host in &hosts {
        let service = a.service.clone();
        let host = *host;
        passes.push(tokio::spawn(
            async move { service.reconcile_host(host).await },
        ));
    }
    for _ in 0..5000 {
        if asked(&a).len() >= MAX_RECONCILE_PASSES {
            break;
        }
        tokio::task::yield_now().await;
    }
    for _ in 0..500 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        asked(&a).len(),
        MAX_RECONCILE_PASSES,
        "hello-triggered passes are bounded by the same pool",
    );

    gate.add_permits(hosts.len());
    for pass in passes {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;
    }
    for action in actions {
        assert_eq!(
            a.get(action.id()).await.status(),
            &ActionStatus::Succeeded,
            "every session's pass runs once a slot frees",
        );
    }
}

/// Waits until the reconciliation scheduler has taken at least `count` hosts from its
/// waiting set. This is observable state, not a sleep: it proves the scheduler considered a
/// trigger before the test changes the pool.
async fn wait_for_scheduler_attempts(a: &Actions, count: u64) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while a.service.reconcile_attempts() < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the scheduler never considered the pending trigger");
}

/// Runs one approved action on `host` whose reply is lost, so the executor asks for a pass.
/// Returns the run's id once the request has been handed over.
async fn lose_a_reply_on(a: &Actions, host: HostId) -> ActionId {
    let before = a.runs();
    let action = a
        .service
        .propose(proposed_for_host(a.incident.id(), host))
        .await
        .unwrap();
    approve(a, &action).await;
    for _ in 0..2000 {
        if a.runs() > before {
            return action.id();
        }
        tokio::task::yield_now().await;
    }
    panic!("the run on {host} was never handed over");
}

/// Five hosts, four of them holding a hello pass in its query. The fifth loses a reply and
/// waits for a slot; releasing the hello passes must wake the scheduler without any further
/// trigger, reconnect or unrelated notification.
#[tokio::test]
async fn a_pending_scheduler_pass_is_reawakened_when_hello_passes_release_slots() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    for _ in 0..4 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    let mut waiting = Vec::new();
    for host in &hosts[..4] {
        let epoch = world.link.journal_epoch(*host).expect("a journal");
        let action = dispatched_to(world.incident.id(), *host, Some(epoch));
        seed_in(&world.store, &action).await;
        waiting.push(action.id());
    }
    let gate = Arc::new(Semaphore::new(0));
    for host in &hosts[..4] {
        world.link.hold_status_queries(*host, gate.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    let mut passes = Vec::new();
    for host in &hosts[..4] {
        let service = a.service.clone();
        let host = *host;
        passes.push(tokio::spawn(
            async move { service.reconcile_host(host).await },
        ));
    }
    for _ in 0..5000 {
        if asked(&a).len() >= MAX_RECONCILE_PASSES {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        asked(&a).len(),
        MAX_RECONCILE_PASSES,
        "four hello passes hold every pass slot",
    );

    let fifth = hosts[4];
    let fifth_run = lose_a_reply_on(&a, fifth).await;
    // A second lost reply for the same waiting host must coalesce into the same pass.
    let fifth_second = lose_a_reply_on(&a, fifth).await;
    wait_for_scheduler_attempts(&a, 1).await;
    assert!(
        asked_of(&a, fifth).is_empty(),
        "the fifth host waits while hello passes hold every slot",
    );
    for id in [fifth_run, fifth_second] {
        assert!(
            matches!(a.get(id).await.status(), ActionStatus::Dispatched { .. }),
            "a lost reply is not an outcome yet",
        );
    }

    gate.add_permits(4);
    for id in [fifth_run, fifth_second] {
        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            a.until(id, |status| matches!(status, ActionStatus::Succeeded)),
        )
        .await
        .expect("the fifth host was never reawakened");
        assert_eq!(settled.status(), &ActionStatus::Succeeded);
    }
    let queries = asked_of(&a, fifth);
    assert_eq!(queries.len(), 1, "the reawakened scheduler asks once");
    assert_eq!(
        queries[0].len(),
        2,
        "both triggers for the waiting host coalesce into the one pass",
    );
    for id in waiting {
        assert_eq!(
            a.get(id).await.status(),
            &ActionStatus::Succeeded,
            "the hello passes settled the runs they held",
        );
    }
    for pass in passes {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), pass).await;
    }
}

/// A hello pass cancelled by its session ending releases its slot and wakes the scheduler
/// the same way a completed one does.
#[tokio::test]
async fn a_cancelled_hello_pass_releases_its_slot_and_wakes_the_scheduler() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    for _ in 0..4 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    for host in &hosts[..4] {
        let epoch = world.link.journal_epoch(*host).expect("a journal");
        let action = dispatched_to(world.incident.id(), *host, Some(epoch));
        seed_in(&world.store, &action).await;
    }
    let gate = Arc::new(Semaphore::new(0));
    for host in &hosts[..4] {
        world.link.hold_status_queries(*host, gate.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    let mut passes = Vec::new();
    for host in &hosts[..4] {
        let service = a.service.clone();
        let host = *host;
        passes.push(tokio::spawn(
            async move { service.reconcile_host(host).await },
        ));
    }
    for _ in 0..5000 {
        if asked(&a).len() >= MAX_RECONCILE_PASSES {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(asked(&a).len(), MAX_RECONCILE_PASSES);

    let fifth = hosts[4];
    let fifth_run = lose_a_reply_on(&a, fifth).await;
    wait_for_scheduler_attempts(&a, 1).await;
    assert!(asked_of(&a, fifth).is_empty());

    let cancelled = passes.remove(0);
    cancelled.abort();
    let _ = cancelled.await;

    let settled = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        a.until(fifth_run, |status| {
            matches!(status, ActionStatus::Succeeded)
        }),
    )
    .await
    .expect("a cancelled hello pass did not release its slot to the scheduler");
    assert_eq!(settled.status(), &ActionStatus::Succeeded);
    for pass in passes {
        pass.abort();
    }
}

/// A scheduler-owned pass releasing its slot also wakes the next hosts. Two hosts wait while
/// the pool is full; each runs in turn as the slot is handed on.
#[tokio::test]
async fn a_scheduler_pass_releasing_its_slot_wakes_the_next_hosts() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    for _ in 0..5 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    for host in &hosts[..3] {
        let epoch = world.link.journal_epoch(*host).expect("a journal");
        let action = dispatched_to(world.incident.id(), *host, Some(epoch));
        seed_in(&world.store, &action).await;
    }
    let held = Arc::new(Semaphore::new(0));
    for host in &hosts[..3] {
        world.link.hold_status_queries(*host, held.clone());
    }
    let slow_slot = Arc::new(Semaphore::new(0));
    world.link.hold_status_queries(hosts[3], slow_slot.clone());
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    let mut hello_passes = Vec::new();
    for host in &hosts[..3] {
        let service = a.service.clone();
        let host = *host;
        hello_passes.push(tokio::spawn(
            async move { service.reconcile_host(host).await },
        ));
    }
    for _ in 0..5000 {
        if asked(&a).len() >= 3 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(asked(&a).len(), 3, "three hello passes hold three slots");

    // The fourth slot goes to a scheduler pass, held inside its query.
    let scheduler_run = lose_a_reply_on(&a, hosts[3]).await;
    for _ in 0..5000 {
        if asked_of(&a, hosts[3]).len() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        asked_of(&a, hosts[3]).len(),
        1,
        "the scheduler pass is held"
    );

    // Two more hosts ask for a pass while the pool is full.
    let first_waiting = lose_a_reply_on(&a, hosts[4]).await;
    let second_waiting = lose_a_reply_on(&a, hosts[5]).await;
    wait_for_scheduler_attempts(&a, 3).await;
    assert!(
        asked_of(&a, hosts[4]).is_empty() && asked_of(&a, hosts[5]).is_empty(),
        "both hosts wait while the pool is full",
    );

    slow_slot.add_permits(1);
    for id in [scheduler_run, first_waiting, second_waiting] {
        let settled = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            a.until(id, |status| matches!(status, ActionStatus::Succeeded)),
        )
        .await
        .expect("a waiting host was not woken by the released slot");
        assert_eq!(settled.status(), &ActionStatus::Succeeded);
    }
    for pass in hello_passes {
        pass.abort();
    }
}

/// A hello pass with no pending rows returns early; its released slot wakes the scheduler
/// for a host that is waiting.
#[tokio::test]
async fn a_hello_pass_with_nothing_to_do_releases_its_slot_for_a_waiting_host() {
    let world = seeded().await;
    let store = world.store.clone();
    let gated = Arc::new(GatedStore::new(store.clone()));
    let mut hosts = vec![host_id()];
    for _ in 0..4 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    for host in &hosts[..3] {
        let epoch = world.link.journal_epoch(*host).expect("a journal");
        let action = dispatched_to(world.incident.id(), *host, Some(epoch));
        seed_in(&world.store, &action).await;
    }
    let held = Arc::new(Semaphore::new(0));
    for host in &hosts[..3] {
        world.link.hold_status_queries(*host, held.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world
        .with_actions(gated.clone())
        .start(ActionSettings::default(), store);

    let mut hello_passes = Vec::new();
    for host in &hosts[..3] {
        let service = a.service.clone();
        let host = *host;
        hello_passes.push(tokio::spawn(
            async move { service.reconcile_host(host).await },
        ));
    }
    for _ in 0..5000 {
        if asked(&a).len() >= 3 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(asked(&a).len(), 3);

    // The fourth slot goes to a hello pass with no rows, held while it reads the listing.
    let listing = gated.hold_dispatched_listing();
    let empty = hosts[3];
    let service = a.service.clone();
    hello_passes.push(tokio::spawn(
        async move { service.reconcile_host(empty).await },
    ));
    listing.reached().await;

    let fifth = hosts[4];
    let fifth_run = lose_a_reply_on(&a, fifth).await;
    wait_for_scheduler_attempts(&a, 1).await;
    assert!(asked_of(&a, fifth).is_empty());

    listing.release();
    let settled = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        a.until(fifth_run, |status| {
            matches!(status, ActionStatus::Succeeded)
        }),
    )
    .await
    .expect("an early-returning hello pass did not release its slot to the scheduler");
    assert_eq!(settled.status(), &ActionStatus::Succeeded);
    for pass in hello_passes {
        pass.abort();
    }
}

/// Waits until a hello pass has started waiting for a host gate another pass holds.
async fn wait_for_gate_wait(a: &Actions) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while a.service.reconcile_gate_waits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("no hello pass ever waited for a held host gate");
}

/// Waits until a hello pass holds its host gate and is waiting for a free pass slot.
async fn wait_for_slot_wait(a: &Actions) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while a.service.reconcile_slot_waits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("no hello pass ever waited for a free pass slot");
}

/// A hello pass waits for its host's gate without holding a global slot, so another host can
/// still use the last slot while that wait lasts.
#[tokio::test]
async fn a_hello_pass_waiting_for_its_host_gate_does_not_reserve_a_global_slot() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    for _ in 0..4 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    let held = Arc::new(Semaphore::new(0));
    for host in &hosts[..3] {
        world.link.hold_status_queries(*host, held.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    // Three scheduler passes (A, B, C) hold three slots inside their queries.
    let mut runs = Vec::new();
    for host in &hosts[..3] {
        runs.push(lose_a_reply_on(&a, *host).await);
    }
    for _ in 0..5000 {
        if asked(&a).len() >= 3 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        asked(&a).len(),
        3,
        "three scheduler passes hold three slots"
    );

    // A reconnects while its scheduler pass still owns A's host gate. The hello pass must
    // wait for that gate without taking the fourth slot.
    let service = a.service.clone();
    let reconnecting = hosts[0];
    let hello = tokio::spawn(async move { service.reconcile_host(reconnecting).await });
    wait_for_gate_wait(&a).await;
    assert!(
        !hello.is_finished(),
        "the hello pass is waiting for A's gate"
    );

    // D is scheduled while the hello pass waits. It must get the free fourth slot.
    let d_run = lose_a_reply_on(&a, hosts[3]).await;
    let settled = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        a.until(d_run, |status| matches!(status, ActionStatus::Succeeded)),
    )
    .await
    .expect("D never got the last slot while the hello pass waited for its host gate");
    assert_eq!(settled.status(), &ActionStatus::Succeeded);
    assert_eq!(
        asked_of(&a, hosts[3]).len(),
        1,
        "the fourth slot went to D, not to the waiting hello pass",
    );

    held.add_permits(3);
    for id in runs {
        a.until(id, |status| matches!(status, ActionStatus::Succeeded))
            .await;
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), hello).await;
}

/// Cancelling a hello pass while it waits for a held host gate frees nothing it held: no
/// global slot is leaked and the last slot stays usable for another host.
#[tokio::test]
async fn cancelling_a_hello_pass_waiting_for_its_host_gate_keeps_the_pool_usable() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    for _ in 0..4 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    let held = Arc::new(Semaphore::new(0));
    for host in &hosts[..3] {
        world.link.hold_status_queries(*host, held.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    let mut runs = Vec::new();
    for host in &hosts[..3] {
        runs.push(lose_a_reply_on(&a, *host).await);
    }
    for _ in 0..5000 {
        if asked(&a).len() >= 3 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(asked(&a).len(), 3);

    let service = a.service.clone();
    let reconnecting = hosts[0];
    let hello = tokio::spawn(async move { service.reconcile_host(reconnecting).await });
    wait_for_gate_wait(&a).await;
    hello.abort();
    let _ = hello.await;

    // The abort did not consume the fourth slot, so D still starts.
    let d_run = lose_a_reply_on(&a, hosts[3]).await;
    let settled = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        a.until(d_run, |status| matches!(status, ActionStatus::Succeeded)),
    )
    .await
    .expect("a cancelled gate waiter consumed a global slot");
    assert_eq!(settled.status(), &ActionStatus::Succeeded);

    held.add_permits(3);
    for id in runs {
        a.until(id, |status| matches!(status, ActionStatus::Succeeded))
            .await;
    }
}

/// Cancelling a hello pass that holds its host gate while every slot is taken releases the
/// gate, so the host's own scheduler trigger still runs once a slot frees.
#[tokio::test]
async fn cancelling_a_hello_pass_that_waits_for_a_slot_releases_its_host_gate() {
    let world = seeded().await;
    let mut hosts = vec![host_id()];
    for _ in 0..4 {
        let host = HostId::new();
        connect_host(&world, host, JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
        hosts.push(host);
    }
    let held = Arc::new(Semaphore::new(0));
    for host in &hosts[..4] {
        world.link.hold_status_queries(*host, held.clone());
    }
    world
        .link
        .on_skill(|_, _| Err(LinkError::Transport("the reply was dropped".to_owned())));
    world.link.on_action_status(|_, ids, epoch| {
        Ok(ActionAnswers {
            epoch,
            answers: ids.iter().map(|id| finished(*id)).collect(),
        })
    });
    let a = world.start(ActionSettings::default(), world.store.clone());

    // Four scheduler passes hold every slot.
    let mut runs = Vec::new();
    for host in &hosts[..4] {
        runs.push(lose_a_reply_on(&a, *host).await);
    }
    for _ in 0..5000 {
        if asked(&a).len() >= MAX_RECONCILE_PASSES {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(asked(&a).len(), MAX_RECONCILE_PASSES);

    // The hello pass takes F's free host gate and then waits for a slot.
    let waiting_host = hosts[4];
    let service = a.service.clone();
    let hello = tokio::spawn(async move { service.reconcile_host(waiting_host).await });
    wait_for_slot_wait(&a).await;

    // F's own scheduler trigger defers behind the hello pass's gate.
    let f_run = lose_a_reply_on(&a, waiting_host).await;
    wait_for_scheduler_attempts(&a, (MAX_RECONCILE_PASSES + 1) as u64).await;
    assert!(asked_of(&a, waiting_host).is_empty());

    hello.abort();
    let _ = hello.await;
    held.add_permits(1);
    let settled = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        a.until(f_run, |status| matches!(status, ActionStatus::Succeeded)),
    )
    .await
    .expect("a cancelled slot waiter left its host gate locked");
    assert_eq!(settled.status(), &ActionStatus::Succeeded);

    held.add_permits(MAX_RECONCILE_PASSES);
    for id in runs {
        a.until(id, |status| matches!(status, ActionStatus::Succeeded))
            .await;
    }
}
