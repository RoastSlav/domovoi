//! The writer's guarded commit for a hand-off the hub proved failed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionRepo, AggregateRef, AuditFilter, AuditLog, IncidentRepo, JournalRead, Page,
};
use domovoi_core::{
    ActionRequest, ActionStatus, HostId, JournalEpoch, KeyFingerprint, Principal, Risk, UserId,
};
use domovoi_testkit::approvals::{approve, human};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::link::action_caps;
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::SignedDuration;
use tokio::sync::{Notify, broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use super::answer::{HANDOFF_FAILED_REASON, HUB_NOT_HANDED_OFF_TOKEN};
use super::{ReconcileOutcome, Shared};
use crate::EnginePolicy;
use crate::PatternRedactor;
use crate::actions::audit;
use crate::actions::writer::{Decision, WriteCommand};
use crate::actions::{ActionDeps, ActionSettings};
use crate::manager::{IncidentManager, ManagerDeps};

/// A world for the writer alone, with no executor and no reconciliation worker.
struct Writer {
    store: Arc<InMemoryStore>,
    commands: crate::actions::writer::WriterHandle,
    host: HostId,
}

impl Writer {
    async fn start() -> Writer {
        let store = Arc::new(InMemoryStore::new());
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let (bus, _events) = broadcast::channel(16);
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
        let (commands, incoming) = crate::actions::writer::queue();
        let (deferred, _deferred_incoming) = tokio::sync::mpsc::unbounded_channel();
        let caps = action_caps(link);
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
                capabilities: Arc::new(domovoi_testkit::FakeCapabilityRegistry::empty()),
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
            reconcile_slots: Arc::new(tokio::sync::Semaphore::new(super::MAX_RECONCILE_PASSES)),
            reconcile_attempts: std::sync::atomic::AtomicU64::new(0),
            reconcile_gate_waits: std::sync::atomic::AtomicU64::new(0),
            reconcile_slot_waits: std::sync::atomic::AtomicU64::new(0),
            reconcile_nudge: Arc::new(Notify::new()),
            deferred,
            writer: commands.clone(),
            catch_up_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            effects_nudge: Arc::new(Notify::new()),
        });
        tokio::spawn(crate::actions::writer::run(shared.clone(), incoming));
        Writer {
            store,
            commands,
            host: HostId::new(),
        }
    }

    /// Stores an incident and one approved, dispatched `restart_container` for it.
    async fn dispatched(&self) -> domovoi_core::ActionId {
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
        let plan = action.plan_hash_for_approval();
        action
            .dispatch(
                plan,
                Some(JournalEpoch::from_uuid(uuid::Uuid::now_v7())),
                T0,
            )
            .unwrap();
        ActionRepo::insert(&*self.store, &action).await.unwrap();
        AuditLog::append(&*self.store, &audit::dispatched(&action, T0).into_event())
            .await
            .unwrap();
        action.id()
    }

    /// Queues one reconciliation outcome and answers with the writer's decision.
    async fn decide(&self, id: domovoi_core::ActionId, outcome: ReconcileOutcome) -> Decision {
        let wire_reason = outcome.token();
        let (reply, answer) = oneshot::channel();
        self.commands
            .tx
            .send(WriteCommand::Reconcile {
                id,
                outcome,
                wire_reason,
                reply,
            })
            .await
            .unwrap();
        answer.await.unwrap()
    }

    async fn status(&self, id: domovoi_core::ActionId) -> ActionStatus {
        ActionRepo::get(&*self.store, id)
            .await
            .unwrap()
            .unwrap()
            .status()
            .clone()
    }

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

    async fn journal_reason(&self, id: domovoi_core::ActionId) -> Option<String> {
        let rows =
            JournalRead::entries_for(&*self.store, &AggregateRef::action(id), Page::MAX_LIMIT)
                .await
                .unwrap();
        rows.last()
            .and_then(|row| row.payload.get("outcome_reason"))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    }
}

#[tokio::test]
async fn a_failed_hand_off_settles_the_run_as_never_started() {
    let writer = Writer::start().await;
    let id = writer.dispatched().await;

    let decided = writer
        .decide(id, ReconcileOutcome::HandoffFailed)
        .await
        .expect("the writer settled the run");

    assert_eq!(
        decided.status(),
        &ActionStatus::NotRun {
            reason: HANDOFF_FAILED_REASON.to_owned(),
        },
        "the hub's proof that nothing was queued is positive evidence of non-execution",
    );
    assert_eq!(
        writer.trail(id).await,
        [audit::DISPATCHED.to_owned(), audit::NOT_RUN.to_owned()],
        "the run is audited once, as one that never happened",
    );
    assert_eq!(
        writer.journal_reason(id).await.as_deref(),
        Some(HUB_NOT_HANDED_OFF_TOKEN),
        "the journal keeps the machine-readable reason",
    );
}

#[tokio::test]
async fn a_failed_hand_off_never_overwrites_a_terminal_decision() {
    let writer = Writer::start().await;
    let id = writer.dispatched().await;
    // Another decision settles the run first.
    let mut action = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
    action
        .never_ran("a person closed it".to_owned(), T0)
        .unwrap();
    ActionRepo::update_from(&*writer.store, &action, "dispatched")
        .await
        .unwrap();
    AuditLog::append(
        &*writer.store,
        &audit::not_run(&action, "a person closed it", T0).into_event(),
    )
    .await
    .unwrap();

    let refused = writer.decide(id, ReconcileOutcome::HandoffFailed).await;

    assert!(
        refused.is_err(),
        "the guarded write refuses a run that is no longer dispatched",
    );
    assert_eq!(
        writer.status(id).await,
        ActionStatus::NotRun {
            reason: "a person closed it".to_owned(),
        },
        "the terminal decision stands",
    );
    assert_eq!(
        writer.trail(id).await,
        [audit::DISPATCHED.to_owned(), audit::NOT_RUN.to_owned()],
        "the refused write adds no audit row",
    );
}
