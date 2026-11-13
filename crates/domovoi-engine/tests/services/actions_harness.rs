//! A running incident manager and action service over fakes, with one open incident on a
//! host that offers `restart_container`.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AuditFilter, AuditLog, CheckRecheck, Clock, IncidentRepo, Page, Presence,
    SkillOutput, UnitOfWork,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, Evidence, Incident, Principal, Risk, SignalStatus,
    SkillSummary, VerifySpec,
};
use domovoi_engine::actions::audit;
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_engine::{
    ActionDeps, ActionHandle, ActionService, ActionSettings, ApprovalError, DomainEvent,
    EnginePolicy, IncidentManager, ManagerDeps, ManagerHandle, PatternRedactor,
};
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::harness::{SECRET, host_id, restart_skill};

pub struct Actions {
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    pub service: ActionHandle,
    pub events: broadcast::Receiver<DomainEvent>,
    pub bus: broadcast::Sender<DomainEvent>,
    pub incident: Incident,
    pub shutdown: CancellationToken,
}

impl Drop for Actions {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// `restart_container` that re-checks after 10 s.
pub fn verified_restart() -> SkillSummary {
    let mut skill = restart_skill();
    skill.verify = Some(VerifySpec {
        recheck: true,
        delay: SignedDuration::from_secs(10),
    });
    skill
}

/// A destructive skill without parameters.
pub fn wipe() -> SkillSummary {
    domovoi_testkit::build::manifest("wipe_volume", Risk::Destructive)
        .unwrap()
        .summary()
}

pub fn ok_output(stdout: &str) -> SkillOutput {
    SkillOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        stderr: String::new(),
        truncated: false,
        duration_ms: 1200,
        argv_hash: [0xab; 32],
    }
}

pub fn check(status: SignalStatus) -> CheckRecheck {
    CheckRecheck {
        status,
        summary: "nginx is running".to_owned(),
        evidence: Evidence::new(),
        observed_at: T0,
    }
}

/// Everything a world holds before the action service starts, so a test can seed what a
/// previous hub session left behind and then start the service over it.
pub struct Seeded {
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    pub bus: broadcast::Sender<DomainEvent>,
    pub shutdown: CancellationToken,
    pub incident: Incident,
    pub manager: ManagerHandle,
}

/// A world over a fresh in-memory store, with the service not started yet.
pub async fn seeded() -> Seeded {
    seeded_over(Arc::new(InMemoryStore::new())).await
}

async fn seeded_over(store: Arc<InMemoryStore>) -> Seeded {
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    link.set_skills(host_id(), vec![verified_restart(), wipe()]);
    link.on_skill(|_, _| Ok(ok_output("restarted nginx")));
    link.on_check(|_, _, _| Ok(check(SignalStatus::Resolved)));
    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    let (bus, _events) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, _task) = IncidentManager::spawn(ManagerDeps {
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
    Seeded {
        store,
        clock,
        link,
        bus,
        shutdown,
        incident,
        manager,
    }
}

impl Seeded {
    /// Starts an action service over this world, committing every action write through
    /// `uow`. Each start gets its own shutdown token, so a test can stop one service and
    /// start another over the same store.
    pub fn start(&self, settings: ActionSettings, uow: Arc<dyn UnitOfWork>) -> Actions {
        let shutdown = self.shutdown.child_token();
        let events = self.bus.subscribe();
        let deps = ActionDeps {
            actions: self.store.clone(),
            incidents: self.store.clone(),
            audit: self.store.clone(),
            uow,
            link: self.link.clone(),
            redactor: Arc::new(PatternRedactor::new([SecretString::from(
                SECRET.to_owned(),
            )])),
            clock: self.clock.clone(),
            manager: self.manager.clone(),
            bus: self.bus.clone(),
            shutdown: shutdown.clone(),
        };
        let (service, _task) = ActionService::spawn(deps, settings);
        Actions {
            store: self.store.clone(),
            clock: self.clock.clone(),
            link: self.link.clone(),
            service,
            events,
            bus: self.bus.clone(),
            incident: self.incident.clone(),
            shutdown,
        }
    }
}

pub async fn actions(settings: ActionSettings) -> Actions {
    let world = seeded().await;
    world.start(settings, world.store.clone())
}

/// Starts the action service over `store`, committing through `uow` so a test can watch
/// or refuse what it writes. Reads go to `store`, which `uow` must forward to.
pub async fn actions_with_uow(
    store: Arc<InMemoryStore>,
    uow: Arc<dyn UnitOfWork>,
    settings: ActionSettings,
) -> Actions {
    let world = seeded_over(store).await;
    world.start(settings, uow)
}

impl Actions {
    pub fn now(&self) -> Timestamp {
        Clock::now(&*self.clock)
    }

    /// Proposes `skill` with `args` as triage would, open for `ttl`, and returns the action
    /// and its plain chat token.
    pub async fn propose_for(
        &self,
        skill: &SkillSummary,
        args: &[(&str, &str)],
        ttl: SignedDuration,
    ) -> (ActionRequest, String) {
        let (action, token) = self.proposal(skill, args, ttl).await;
        ActionRepo::insert(&*self.store, &action).await.unwrap();
        AuditLog::append(
            &*self.store,
            &audit::proposed(&action, self.now()).into_event(),
        )
        .await
        .unwrap();
        (action, token)
    }

    /// Proposes `skill` with `args` through the writer, as triage now does, and returns
    /// the stored action and its plain chat token.
    pub async fn propose_via_service(
        &self,
        skill: &SkillSummary,
        args: &[(&str, &str)],
        ttl: SignedDuration,
    ) -> Result<(ActionRequest, String), ApprovalError> {
        let (action, token) = self.proposal(skill, args, ttl).await;
        let stored = self.service.propose(action).await?;
        Ok((stored, token))
    }

    pub async fn proposal(
        &self,
        skill: &SkillSummary,
        args: &[(&str, &str)],
        ttl: SignedDuration,
    ) -> (ActionRequest, String) {
        let args: BTreeMap<String, String> = args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let validated = skill.validate_args(&args).unwrap();
        let token = mint_token();
        let action = ActionRequest::propose(
            self.incident.id(),
            host_id(),
            skill,
            &validated,
            Principal::System("triage".into()),
            ttl,
            self.now(),
        )
        .with_chat_token_hash(token_hash(&token));
        (action, token)
    }

    /// Proposes `restart_container` for `nginx`, open for an hour.
    pub async fn propose(&self) -> (ActionRequest, String) {
        self.propose_for(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
    }

    pub async fn get(&self, id: ActionId) -> ActionRequest {
        ActionRepo::get(&*self.store, id).await.unwrap().unwrap()
    }

    /// Lets the executor run, advancing time a second per step, until `done` holds for the
    /// stored action.
    pub async fn until(&self, id: ActionId, done: impl Fn(&ActionStatus) -> bool) -> ActionRequest {
        for _ in 0..2000 {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            let action = self.get(id).await;
            if done(action.status()) {
                return action;
            }
            self.clock.advance(SignedDuration::from_secs(1));
        }
        panic!("action {id} never reached the expected status");
    }

    /// Lets queued work run without moving time.
    pub async fn settle(&self) {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    /// The audit kinds for `id`, oldest first.
    pub async fn trail(&self, id: ActionId) -> Vec<String> {
        let filter = AuditFilter {
            subject: Some(format!("action:{id}")),
            ..AuditFilter::default()
        };
        let mut kinds: Vec<String> = AuditLog::list(&*self.store, &filter, &Page::first(100))
            .await
            .unwrap()
            .items
            .into_iter()
            .map(|e| e.kind)
            .collect();
        kinds.reverse();
        kinds
    }

    /// Every event published so far.
    pub fn drain(&mut self) -> Vec<DomainEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.push(event);
        }
        out
    }

    /// How many skills were dispatched to the agent.
    pub fn runs(&self) -> usize {
        self.link
            .requests()
            .iter()
            .filter(|r| matches!(r, domovoi_testkit::link::LinkRequest::Skill { .. }))
            .count()
    }
}
