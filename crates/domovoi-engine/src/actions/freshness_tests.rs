use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionRepo, AgentLink, AuditFilter, AuditLog, HostRepo, IncidentRepo, Page,
};
use domovoi_core::{
    ActionRequest, ActionStatus, HostId, IncidentEvent, IncidentPolicy, KeyFingerprint, PlanHash,
    PlanTarget, Principal, Risk, UserId, apply,
};
use domovoi_testkit::approvals::{approve, human};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::link::action_caps;
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::SignedDuration;
use tokio::sync::{Notify, broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use crate::EnginePolicy;
use crate::PatternRedactor;
use crate::actions::executor::MAX_CATCH_UP_RUNS;
use crate::actions::writer::{self, WriteCommand};
use crate::actions::{ActionDeps, ActionSettings, Shared};
use crate::manager::{IncidentManager, ManagerDeps};

/// A world for the writer alone: the store, the clock and the fake agent, the writer's
/// queue, and no executor at all. The writer loop is the only thing reading commands,
/// so a dispatch that is refused here was refused by the writer's own check.
struct Writer {
    shared: Arc<Shared>,
    store: Arc<InMemoryStore>,
    clock: Arc<FakeClock>,
    link: Arc<FakeAgentLink>,
    host: HostId,
    commands: writer::WriterHandle,
}

impl Writer {
    async fn start() -> Writer {
        let store = Arc::new(InMemoryStore::new());
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let host = HostId::new();
        let (bus, _events) = broadcast::channel(16);
        let shutdown = CancellationToken::new();
        link.set_presence(
            host,
            domovoi_core::ports::Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        link.set_journal_epoch(
            host,
            domovoi_core::JournalEpoch::from_uuid(uuid::Uuid::now_v7()),
        );
        let mut registered = domovoi_testkit::build::host("freshness").unwrap();
        registered.id = host;
        HostRepo::upsert_host(&*store, &registered).await.unwrap();
        let agent_id = domovoi_core::AgentId::new();
        HostRepo::put_agent(
            &*store,
            &domovoi_core::AgentIdentity {
                agent: agent_id,
                host,
                public_key: [7; 32],
                version: "0.1.0".to_owned(),
                enrolled_at: T0,
                revoked_at: None,
            },
        )
        .await
        .unwrap();
        link.set_agent(host, agent_id);
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
        let (commands, incoming) = writer::queue();
        let (deferred, _deferred_incoming) = tokio::sync::mpsc::unbounded_channel();
        let caps = action_caps(link.clone());
        let shared = Arc::new(Shared {
            deps: ActionDeps {
                actions: store.clone(),
                incidents: store.clone(),
                audit: store.clone(),
                uow: store.clone(),
                effects: store.clone(),
                advisories: store.clone(),
                probe: caps.probe,
                observer: caps.observer,
                journal: caps.journal,
                handoff: caps.handoff,
                rerun: caps.rerun,
                hosts: store.clone(),
                redactor: Arc::new(PatternRedactor::default()),
                clock: clock.clone(),
                manager,
                bus,
                shutdown,
            },
            settings: ActionSettings::default(),
            dispatches: Mutex::new(BTreeMap::new()),
            running: Mutex::new(BTreeSet::new()),
            dispatch_gates: Mutex::new(BTreeMap::new()),
            reconcile_passes: Mutex::new(BTreeMap::new()),
            reconcile_pending: Mutex::new(BTreeSet::new()),
            reconcile_again: Mutex::new(BTreeSet::new()),
            reconcile_slots: Arc::new(tokio::sync::Semaphore::new(
                crate::actions::reconcile::MAX_RECONCILE_PASSES,
            )),
            reconcile_attempts: std::sync::atomic::AtomicU64::new(0),
            reconcile_gate_waits: std::sync::atomic::AtomicU64::new(0),
            reconcile_slot_waits: std::sync::atomic::AtomicU64::new(0),
            reconcile_nudge: Arc::new(Notify::new()),
            deferred,
            writer: commands.clone(),
            catch_up_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CATCH_UP_RUNS)),
            effects_nudge: Arc::new(Notify::new()),
        });
        tokio::spawn(writer::run(shared.clone(), incoming));
        Writer {
            shared,
            store,
            clock,
            link,
            host,
            commands,
        }
    }

    /// Stores an incident on this host and one approved `restart_container` for it,
    /// returning the action's id.
    async fn approved(&self) -> (domovoi_core::ActionId, PlanHash) {
        let incident = open_incident(self.host, "docker.container", Some("nginx")).unwrap();
        IncidentRepo::insert(&*self.store, &incident).await.unwrap();
        let skill = manifest("restart_container", Risk::Write)
            .unwrap()
            .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"));
        let args = skill.validate_args(&BTreeMap::new()).unwrap();
        let mut action = ActionRequest::propose(
            &incident,
            &skill,
            &args,
            Some(KeyFingerprint::of_public_key(&[7u8; 32])),
            Principal::System("triage".into()),
            SignedDuration::from_hours(1),
            T0,
        )
        .unwrap();
        approve(&mut action, human(UserId::new()), T0).unwrap();
        ActionRepo::insert(&*self.store, &action).await.unwrap();
        let plan = action.plan_hash_for_approval();
        (action.id(), plan)
    }

    /// Sends a `Dispatch` command to the writer and answers with what it decided.
    async fn dispatch(&self, id: domovoi_core::ActionId, plan: PlanHash) -> ActionRequest {
        let stored = ActionRepo::get(&*self.store, id)
            .await
            .unwrap()
            .expect("the action is stored");
        let incident = IncidentRepo::get(&*self.store, stored.incident())
            .await
            .unwrap()
            .expect("the incident is stored");
        let agent = HostRepo::live_agent(&*self.store, self.host)
            .await
            .unwrap()
            .expect("the harness host has an agent");
        let target = stored.plan_hash_v2().map(|_| {
            PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&agent.public_key))
        });
        let snapshot = self
            .link
            .snapshot(self.host)
            .expect("the harness host has a session");
        let (reply, answer) = oneshot::channel();
        self.commands
            .tx
            .send(WriteCommand::Dispatch {
                id,
                plan,
                target,
                session: snapshot.session,
                epoch: snapshot.journal_epoch,
                reply,
            })
            .await
            .unwrap();
        answer.await.unwrap().expect("the writer answered")
    }

    /// The audit kinds recorded for `id`, oldest first.
    async fn trail(&self, id: domovoi_core::ActionId) -> Vec<String> {
        let filter = AuditFilter {
            subject: Some(format!("action:{id}")),
            ..AuditFilter::default()
        };
        let mut kinds: Vec<String> = AuditLog::list(&*self.store, &filter, &Page::first(100))
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|entry| entry.kind)
            .collect();
        kinds.reverse();
        kinds
    }
}

#[tokio::test]
async fn writer_settles_a_dispatch_whose_approval_expired_as_never_run() {
    let writer = Writer::start().await;
    let (id, plan) = writer.approved().await;
    // The window closes while the dispatch waits for the writer.
    writer.clock.advance(SignedDuration::from_hours(2));

    let decided = writer.dispatch(id, plan).await;
    assert_eq!(
        decided.status(),
        &ActionStatus::NotRun {
            reason: "the approval expired before the action was dispatched".to_owned(),
        },
        "the writer itself settles a stale approval, without the executor's help",
    );
    let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
    assert_eq!(
        stored.status(),
        decided.status(),
        "the decision reached the store",
    );
    let trail = writer.trail(id).await;
    assert!(
        trail.contains(&"action.not_run".to_owned()),
        "the closed window is audited as a run that never happened: {trail:?}",
    );
    assert!(
        !trail.contains(&"action.dispatched".to_owned()),
        "nothing was dispatched: {trail:?}",
    );
    assert!(
        writer.link.requests().is_empty(),
        "no request reached the agent",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn writer_refuses_an_approval_whose_incident_resolved_before_dispatch() {
    let writer = Writer::start().await;
    let (id, plan) = writer.approved().await;
    let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
    let mut incident = IncidentRepo::get(&*writer.store, stored.incident())
        .await
        .unwrap()
        .unwrap();
    apply(
        &mut incident,
        IncidentEvent::Resolved { observed_at: T0 },
        &IncidentPolicy::default(),
        T0,
    )
    .unwrap();
    IncidentRepo::update(&*writer.store, &incident)
        .await
        .unwrap();

    let decided = writer.dispatch(id, plan).await;
    assert_eq!(
        decided.status(),
        &ActionStatus::Rejected {
            reason: "the incident was resolved before the action was dispatched".to_owned(),
        },
        "an incident that closed while the dispatch waited stops it",
    );
    let trail = writer.trail(id).await;
    assert!(
        !trail.contains(&"action.dispatched".to_owned()),
        "nothing was dispatched: {trail:?}",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn an_expiry_wins_over_a_queued_rejection() {
    let writer = Writer::start().await;
    let (id, _plan) = writer.approved().await;
    // The executor queued a rejection while the window was open; the writer applies it after
    // it closed. Nothing was attempted, so the run ends as one that never happened.
    writer.clock.advance(SignedDuration::from_hours(2));

    let decided =
        super::super::reject::reject(&writer.shared, id, "agent offline".to_owned(), false)
            .await
            .expect("the writer decided");

    assert_eq!(
        decided.status(),
        &ActionStatus::NotRun {
            reason: "the approval expired before the action was dispatched".to_owned(),
        },
        "a closed window outranks the reason the rejection was queued with",
    );
    assert_eq!(
        writer.trail(id).await,
        ["action.not_run".to_owned()],
        "the expiry is audited once, as a run that never happened",
    );
    assert!(
        writer.link.requests().is_empty(),
        "nothing reached the agent",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn an_expiry_wins_over_a_queued_loop_guard_rejection() {
    let writer = Writer::start().await;
    let (id, _plan) = writer.approved().await;
    writer.clock.advance(SignedDuration::from_hours(2));

    let decided = super::super::reject::reject(
        &writer.shared,
        id,
        "loop guard: restart_container already ran 3 times".to_owned(),
        true,
    )
    .await
    .expect("the writer decided");

    assert_eq!(
        decided.status(),
        &ActionStatus::NotRun {
            reason: "the approval expired before the action was dispatched".to_owned(),
        },
        "the loop guard does not relabel a run whose window closed",
    );
    let trail = writer.trail(id).await;
    assert_eq!(trail, ["action.not_run".to_owned()]);
    assert!(
        !trail.contains(&"action.blocked".to_owned()),
        "an expiry is not a block: {trail:?}",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn a_valid_approval_still_rejects_with_its_reason() {
    let writer = Writer::start().await;
    let (id, _plan) = writer.approved().await;

    let decided = super::super::reject::reject(
        &writer.shared,
        id,
        "loop guard: restart_container already ran 3 times".to_owned(),
        true,
    )
    .await
    .expect("the writer decided");

    assert_eq!(
        decided.status(),
        &ActionStatus::Rejected {
            reason: "loop guard: restart_container already ran 3 times".to_owned(),
        },
        "a rejection inside the window keeps the reason it was made for",
    );
    assert_eq!(
        writer.trail(id).await,
        ["action.blocked".to_owned()],
        "and is audited as the block it is",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn an_incident_that_went_away_is_rejected_for_that_reason() {
    let writer = Writer::start().await;
    let (id, _plan) = writer.approved().await;
    let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
    let mut incident = IncidentRepo::get(&*writer.store, stored.incident())
        .await
        .unwrap()
        .unwrap();
    apply(
        &mut incident,
        IncidentEvent::Resolved { observed_at: T0 },
        &IncidentPolicy::default(),
        T0,
    )
    .unwrap();
    IncidentRepo::update(&*writer.store, &incident)
        .await
        .unwrap();

    let decided =
        super::super::reject::reject(&writer.shared, id, "agent offline".to_owned(), false)
            .await
            .expect("the writer decided");

    assert_eq!(
        decided.status(),
        &ActionStatus::Rejected {
            reason: "the incident was resolved before the action was dispatched".to_owned(),
        },
        "the writer names the check that actually stopped it",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn a_freshness_store_error_leaves_the_action_approved() {
    let writer = Writer::start().await;
    let (id, _plan) = writer.approved().await;
    writer
        .store
        .fail_next(domovoi_core::ports::StoreError::Backend("disk".to_owned()));

    let refused =
        super::super::reject::reject(&writer.shared, id, "agent offline".to_owned(), false).await;

    assert!(
        refused.is_err(),
        "a store that cannot answer must not guess an outcome",
    );
    let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
    assert_eq!(
        stored.status(),
        &ActionStatus::Approved,
        "the action is left exactly as it was",
    );
    writer.shared.deps.shutdown.cancel();
}

#[tokio::test]
async fn a_rejection_never_expires_a_dispatched_action() {
    let writer = Writer::start().await;
    let (id, plan) = writer.approved().await;
    writer.dispatch(id, plan).await;
    writer.clock.advance(SignedDuration::from_hours(2));

    let refused =
        super::super::reject::reject(&writer.shared, id, "agent offline".to_owned(), false).await;

    assert!(
        refused.is_err(),
        "a dispatched run is not decided by the expiry rule",
    );
    let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
    assert!(
        matches!(stored.status(), ActionStatus::Dispatched { .. }),
        "the run stays dispatched for reconciliation: {:?}",
        stored.status(),
    );
    writer.shared.deps.shutdown.cancel();
}
