//! Tests for the one execution boundary: who may dispatch, which hash goes on the wire,
//! and what happens when the writer refuses.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{
    ActionRepo, AgentLink, AuditFilter, AuditLog, HostRepo, IncidentRepo, Page, Presence,
    SkillOutput,
};
use domovoi_core::{
    ActionRequest, ActionStatus, AgentId, AgentIdentity, AuthorizationKind, HostId, Incident,
    KeyFingerprint, PlanHash, Principal, Risk, ScriptDigest, SkillSummary, UserId, ValidatedArgs,
};
use domovoi_testkit::approvals::{approve, human};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::link::LinkRequest;
use domovoi_testkit::link::action_caps;
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::SignedDuration;
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;

use super::{AuthorizedExecution, ExecuteError, execute};
use crate::EnginePolicy;
use crate::PatternRedactor;
use crate::actions::executor::MAX_CATCH_UP_RUNS;
use crate::actions::{ActionDeps, ActionSettings, Shared, preflight, writer};
use crate::manager::{IncidentManager, ManagerDeps};

/// A hub with one host, one incident and one agent, and the writer loop answering the
/// boundary's commands. The executor's own task never starts: every dispatch here is asked
/// for directly, so nothing between the preflights and the boundary hides a refusal.
struct World {
    shared: Arc<Shared>,
    store: Arc<InMemoryStore>,
    clock: Arc<FakeClock>,
    link: Arc<FakeAgentLink>,
    host: HostId,
    incident: Incident,
}

impl World {
    async fn start() -> World {
        let store = Arc::new(InMemoryStore::new());
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let host = HostId::new();
        let (bus, _events) = broadcast::channel(16);
        let shutdown = CancellationToken::new();
        link.set_presence(
            host,
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        link.set_journal_epoch(
            host,
            domovoi_core::JournalEpoch::from_uuid(uuid::Uuid::now_v7()),
        );
        let mut registered = domovoi_testkit::build::host("execution").unwrap();
        registered.id = host;
        HostRepo::upsert_host(&*store, &registered).await.unwrap();
        let agent = AgentId::new();
        HostRepo::put_agent(
            &*store,
            &AgentIdentity {
                agent,
                host,
                public_key: [7; 32],
                version: "0.1.0".to_owned(),
                enrolled_at: T0,
                revoked_at: None,
            },
        )
        .await
        .unwrap();
        link.set_agent(host, agent);
        link.set_skills(host, vec![verified_restart()]);
        link.on_skill(|_, _| {
            Ok(SkillOutput {
                exit_code: Some(0),
                stdout: "restarted".to_owned(),
                stderr: String::new(),
                truncated: false,
                duration_ms: 10,
                argv_hash: [0; 32],
            })
        });
        let incident = open_incident(host, "docker.container", Some("nginx")).unwrap();
        IncidentRepo::insert(&*store, &incident).await.unwrap();
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
            writer: commands,
            catch_up_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CATCH_UP_RUNS)),
            effects_nudge: Arc::new(Notify::new()),
        });
        tokio::spawn(writer::run(shared.clone(), incoming));
        World {
            shared,
            store,
            clock,
            link,
            host,
            incident,
        }
    }

    /// A proposed `restart_container` on this world's incident and host.
    fn proposed(&self) -> ActionRequest {
        propose(&self.incident)
    }

    /// Stores one approved `restart_container` for the incident and answers with it.
    async fn approved(&self) -> ActionRequest {
        let mut action = self.proposed();
        approve(&mut action, human(UserId::new()), T0).unwrap();
        ActionRepo::insert(&*self.store, &action).await.unwrap();
        action
    }

    async fn stored(&self, action: &ActionRequest) -> ActionRequest {
        ActionRepo::get(&*self.store, action.id())
            .await
            .unwrap()
            .expect("the action is stored")
    }

    /// Does what the executor does before the boundary, then calls it: the incident, the
    /// agent session and the validated skill as the host has them now.
    async fn execute(&self, action: &ActionRequest) -> Result<(), ExecuteError> {
        let action = self.stored(action).await;
        let incident = IncidentRepo::get(&*self.store, action.incident())
            .await
            .unwrap()
            .expect("the incident is stored");
        let snapshot = self
            .link
            .snapshot(self.host)
            .expect("the host is connected");
        let (skill, args) =
            preflight::revalidate(&snapshot.skills, &action).expect("the skill is on the host");
        let authorized = AuthorizedExecution::of(action)?;
        execute(&self.shared, authorized, skill, args, snapshot, incident).await
    }

    /// The audit kinds recorded for `action`, oldest first.
    async fn trail(&self, action: &ActionRequest) -> Vec<String> {
        let filter = AuditFilter {
            subject: Some(format!("action:{}", action.id())),
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

    fn dispatched_at_all(&self) -> bool {
        self.link
            .requests()
            .iter()
            .any(|request| matches!(request, LinkRequest::Skill { .. }))
    }
}

/// A proposed `restart_container` on the incident's host, with the v2 plan a current
/// proposal carries.
fn propose(incident: &Incident) -> ActionRequest {
    let skill = verified_restart();
    ActionRequest::propose(
        incident,
        &skill,
        &verified_args(),
        Some(KeyFingerprint::of_public_key(&[7; 32])),
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    )
    .unwrap()
}

/// The one skill in the catalogue, with an entry digest so its plan is a v2 plan.
fn verified_restart() -> SkillSummary {
    manifest("restart_container", Risk::Write)
        .unwrap()
        .summary(ScriptDigest::of_bytes(b"fixture entry"))
}

fn verified_args() -> ValidatedArgs {
    verified_restart().validate_args(&BTreeMap::new()).unwrap()
}

#[tokio::test]
async fn execute_refuses_an_action_without_authorization() {
    let world = World::start().await;
    let action = world.proposed();
    let refused = AuthorizedExecution::of(action).unwrap_err();
    assert_eq!(refused, ExecuteError::NotAuthorized);
    assert!(!world.dispatched_at_all(), "nothing was sent");
}

#[tokio::test]
async fn execute_refuses_a_policy_authorization() {
    let world = World::start().await;
    let action = world.approved().await;
    let row = policy_row(&world, &action);
    let loaded = ActionRequest::try_from_stored(row).expect("an approved policy row is loadable");
    let refused = AuthorizedExecution::of(loaded).unwrap_err();
    assert_eq!(refused, ExecuteError::NotYetSupported);
    assert!(!world.dispatched_at_all(), "nothing was sent");
    assert_eq!(
        world.stored(&action).await.status(),
        &ActionStatus::Approved,
        "the refusal writes nothing"
    );
}

#[tokio::test]
async fn execute_refuses_an_expired_approval() {
    let world = World::start().await;
    let action = world.approved().await;
    world.clock.advance(SignedDuration::from_hours(2));
    let refused = world.execute(&action).await.unwrap_err();
    assert_eq!(refused, ExecuteError::Expired);
    assert!(!world.dispatched_at_all(), "nothing was sent");
    assert_eq!(
        world.stored(&action).await.status(),
        &ActionStatus::Approved,
        "the guard writes no outcome of its own"
    );
    assert!(
        !world
            .trail(&action)
            .await
            .contains(&"action.dispatched".to_owned()),
        "no dispatch was audited"
    );
}

#[tokio::test]
async fn executor_preflights_resolve_expiry_before_execute() {
    let world = World::start().await;
    let action = world.approved().await;
    world.clock.advance(SignedDuration::from_hours(2));
    crate::actions::executor::run(&world.shared, action.id(), None).await;
    assert_eq!(
        world.stored(&action).await.status(),
        &ActionStatus::NotRun {
            reason: "the approval expired before the action was dispatched".to_owned(),
        },
        "the writer settles the closed window as never run"
    );
    let trail = world.trail(&action).await;
    assert_eq!(
        trail
            .iter()
            .filter(|kind| *kind == "action.not_run")
            .count(),
        1,
        "exactly one audit entry: {trail:?}"
    );
    assert!(!world.dispatched_at_all(), "no agent request");
    assert_eq!(
        world.link.snapshot_calls(),
        0,
        "the run stopped before it read the agent session"
    );
}

#[tokio::test]
async fn execute_sends_the_v1_local_check_hash() {
    let world = World::start().await;
    let action = world.approved().await;
    let approval_hash = action.plan_hash_for_approval();
    assert_eq!(
        action.plan_hash_v2(),
        Some(approval_hash),
        "a current proposal binds to a v2 plan"
    );
    world.execute(&action).await.expect("the run is dispatched");

    let sent = world
        .link
        .requests()
        .into_iter()
        .find_map(|request| match request {
            LinkRequest::Skill { dispatch, .. } => Some(dispatch),
            _ => None,
        })
        .expect("the agent saw the run");
    let skill = verified_restart();
    let v1 = PlanHash::compute(
        &skill.name,
        skill.manifest_hash,
        world.host,
        &verified_args(),
    );
    assert_eq!(
        sent.plan_hash, v1,
        "the wire carries the v1 hash the agent recomputes"
    );
    assert_ne!(
        sent.plan_hash, approval_hash,
        "the v2 approval hash is never put on the wire"
    );
}

#[tokio::test]
async fn execute_does_not_run_a_skill_the_writer_did_not_dispatch() {
    let world = World::start().await;
    let action = world.approved().await;
    // The host's skill is not the one the approval covered any more.
    let changed = manifest("restart_container", Risk::Write)
        .unwrap()
        .summary(ScriptDigest::of_bytes(b"a different entry"));
    world.link.set_skills(world.host, vec![changed]);

    let refused = world.execute(&action).await.unwrap_err();
    assert_eq!(refused, ExecuteError::PlanChanged);
    assert!(!world.dispatched_at_all(), "the recording link saw no call");
    assert!(
        matches!(
            world.stored(&action).await.status(),
            ActionStatus::Rejected { .. }
        ),
        "the writer rejected the changed plan"
    );
    assert!(
        !world
            .trail(&action)
            .await
            .contains(&"action.dispatched".to_owned()),
        "no dispatch was audited"
    );
}

#[tokio::test]
async fn execute_refuses_a_loop_guard_slot_it_cannot_take() {
    let world = World::start().await;
    let action = world.approved().await;
    // Every slot of the rolling hour is taken by this same skill on this host.
    for _ in 0..world.shared.settings.loop_guard_per_hour {
        assert!(
            preflight::reserve_dispatch(&world.shared, &action, T0).is_none(),
            "the guard still has a slot"
        );
    }
    let refused = world.execute(&action).await.unwrap_err();
    assert!(
        matches!(refused, ExecuteError::Refused(ref reason) if reason.contains("loop guard")),
        "the guard's own reason is the refusal: {refused:?}"
    );
    assert!(!world.dispatched_at_all(), "nothing was sent");
    assert!(
        matches!(
            world.stored(&action).await.status(),
            ActionStatus::Rejected { reason } if reason.contains("loop guard")
        ),
        "the writer rejected the run before dispatch"
    );
}

/// The approved row of `action`, as a stored row a policy authorization would produce.
fn policy_row(world: &World, action: &ActionRequest) -> domovoi_core::StoredAction {
    domovoi_core::StoredAction {
        id: action.id(),
        incident: action.incident(),
        host: world.host,
        skill: action.skill().clone(),
        risk: action.risk(),
        args: action.args().clone(),
        plan_hash: action.plan_hash(),
        plan_hash_v2: None,
        approved_plan_hash: None,
        provenance: None,
        authorization_kind: Some(AuthorizationKind::Policy),
        status: ActionStatus::Approved,
        proposed_by: action.proposed_by().clone(),
        approved_by: None,
        expires_at: action.expires_at(),
        created_at: action.created_at(),
        updated_at: action.updated_at(),
        chat_token_hash: None,
    }
}
