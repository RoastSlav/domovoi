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
    ActionAnswer, ActionAnswerEntry, ActionAnswers, ActionFilter, ActionRepo, AuditFilter,
    AuditLog, HostReconciler, LinkError, Page, Paged, RAN_STATUSES, StoreError, UnknownReason,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, IncidentId, JournalEpoch, PlanBinding, PlanHash,
    Principal, UserId,
};
use domovoi_engine::ActionSettings;
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

#[tokio::test]
async fn a_full_agent_queue_is_not_a_failure() {
    // The request never left the hub. A legacy agent cannot prove it never arrived, so the
    // journal answer is an unknown outcome, and never a failure.
    let world = seeded().await;
    world.link.fail_next_skill_handoff(LinkError::Rejected(
        "the agent already has 256 requests in flight".to_owned(),
    ));
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
        "a request that was never handed over is not a failure: {:?}",
        done.status(),
    );
    assert_eq!(a.runs(), 0, "nothing reached the agent");
    assert_eq!(
        a.journal_reason(action.id()).await.as_deref(),
        Some("journal_lost")
    );
}

#[tokio::test]
async fn a_request_that_never_left_the_hub_is_not_run_when_absence_is_authoritative() {
    // The same case with an agent that journals every execution: a matching-epoch
    // `not_found` is positive evidence and settles the run as never started.
    let world = seeded().await;
    world.link.fail_next_skill_handoff(LinkError::Rejected(
        "the agent already has 256 requests in flight".to_owned(),
    ));
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    let done = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::NotRun { .. })
        })
        .await;

    assert!(
        matches!(done.status(), ActionStatus::NotRun { .. }),
        "an authoritative empty journal says the run never arrived: {:?}",
        done.status(),
    );
    assert_eq!(a.runs(), 0, "nothing reached the agent");
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
            ActionStatus::Dispatched { .. }
        ),
        "the run stays dispatched for the new session to settle",
    );
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
