//! A running incident manager and action service over fakes, with one open incident on a
//! host that offers `restart_container`.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AuditFilter, AuditLog, CheckRecheck, Clock, IncidentRepo, Page, Presence,
    SkillOutput,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, Evidence, Incident, Principal, Risk, SignalStatus,
    SkillSummary, VerifySpec,
};
use domovoi_engine::actions::audit;
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_engine::{
    ActionDeps, ActionHandle, ActionService, ActionSettings, DomainEvent, EnginePolicy,
    IncidentManager, ManagerDeps, PatternRedactor,
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

pub async fn actions(settings: ActionSettings) -> Actions {
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
    link.set_skills(host_id(), vec![verified_restart(), wipe()]);
    link.on_skill(|_, _| Ok(ok_output("restarted nginx")));
    link.on_check(|_, _, _| Ok(check(SignalStatus::Resolved)));
    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*store, &incident).await.unwrap();
    let (bus, events) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, _task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let deps = ActionDeps {
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
    };
    let (service, _task) = ActionService::spawn(deps, settings);
    Actions {
        store,
        clock,
        link,
        service,
        events,
        bus,
        incident,
        shutdown,
    }
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
        ActionRepo::insert(&*self.store, &action).await.unwrap();
        AuditLog::append(&*self.store, &audit::proposed(&action, self.now()))
            .await
            .unwrap();
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
