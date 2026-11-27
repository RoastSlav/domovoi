//! A running incident manager and triage service over fakes, with one open incident.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    Clock, HostRepo, IncidentTimelineEntry, LlmProvider, LlmResponse, NoteRepo, Part, Presence,
    SignalRepo, StopReason, ToolOutput, Usage,
};
use domovoi_core::{
    AgentIdentity, CheckKey, Evidence, EvidenceValue, HostId, Incident, Note, NoteId, ParamKind,
    ParamName, ParamSpec, Risk, Severity, Signal, SkillSummary, SourceKind, TargetKey,
};
use domovoi_engine::context::ExperienceSource;
use domovoi_engine::operations::{
    InvestigateDeps, InvestigateSettings, OperationDeps, OperationHandle, OperationService,
    OperationSettings,
};
use domovoi_engine::triage::{TriageDeps, TriageHandle, TriageService, TriageSettings};
use domovoi_engine::{DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps};
use domovoi_engine::{ManagerHandle, PatternRedactor};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore, ScriptedLlm};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// A value registered with the redactor that must never reach the model.
pub const SECRET: &str = "hunter2-correct-horse-battery";

pub fn host_id() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

/// How a world is built.
pub struct Setup {
    pub settings: TriageSettings,
    pub secondary: bool,
    pub third: bool,
    pub cli: bool,
    pub evidence: Evidence,
    /// A real provider to use as the primary instead of the scripted one.
    pub provider: Option<Arc<dyn LlmProvider>>,
    /// A usage repository to use instead of the store's, for budget read failures.
    pub usage: Option<Arc<dyn domovoi_core::ports::UsageRepo>>,
    /// An admission repository to use instead of the store's, for window read failures.
    pub admission: Option<Arc<dyn domovoi_core::ports::AdmissionRepo>>,
    /// How long incident listings and note searches take, for deadline tests.
    pub store_delay: Option<std::time::Duration>,
    /// A signal repository to use instead of the store's, for read failures.
    pub signals: Option<Arc<dyn SignalRepo>>,
    /// Extra context sources registered with the triage service.
    pub extra_context: Vec<Arc<dyn domovoi_core::ports::ContextSource>>,
    /// Whether the experience source is also registered as a context source, as the hub does.
    pub with_experience: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            settings: TriageSettings::default(),
            secondary: false,
            third: false,
            cli: false,
            evidence: disk(92.4),
            provider: None,
            usage: None,
            admission: None,
            store_delay: None,
            signals: None,
            extra_context: Vec::new(),
            with_experience: false,
        }
    }
}

pub fn disk(used: f64) -> Evidence {
    let mut evidence = Evidence::new();
    evidence.insert("used_pct", EvidenceValue::Float(used));
    evidence.insert(
        "observed",
        EvidenceValue::Text("2026-10-31T11:52:00Z".to_owned()),
    );
    evidence
}

pub struct World {
    /// The incident manager's task, so a test can stop it.
    pub manager_task: tokio::task::JoinHandle<()>,
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    /// The registry the triage service reads, so a test can switch a skill off.
    pub capabilities: Arc<domovoi_testkit::FakeCapabilityRegistry>,
    pub primary: Arc<ScriptedLlm>,
    pub secondary: Arc<ScriptedLlm>,
    pub third: Arc<ScriptedLlm>,
    pub manager: ManagerHandle,
    pub triage: TriageHandle,
    pub events: broadcast::Receiver<DomainEvent>,
    pub bus: broadcast::Sender<DomainEvent>,
    pub incident: Incident,
    pub shutdown: CancellationToken,
    /// Stops the triage and operation services that started with the world.
    services: CancellationToken,
    /// The operation service that investigates the world's incidents.
    pub ops: OperationHandle,
    /// What the running triage service was built from, so a restart can rebuild it.
    primary_dyn: Arc<dyn LlmProvider>,
    fallbacks: Vec<Arc<dyn LlmProvider>>,
    settings: TriageSettings,
    redactor: Arc<PatternRedactor>,
    usage: Arc<dyn domovoi_core::ports::UsageRepo>,
    admission: Arc<dyn domovoi_core::ports::AdmissionRepo>,
}

impl Drop for World {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// The open `docker.container` incident on `nginx`, with one signal carrying `evidence`.
pub async fn incident_with(store: &InMemoryStore, evidence: Evidence) -> Incident {
    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    domovoi_core::ports::IncidentRepo::insert(store, &incident)
        .await
        .unwrap();
    add_signal(store, &incident, evidence, T0).await;
    incident
}

/// Adds a firing signal observed at `at` to `incident`.
pub async fn add_signal(
    store: &InMemoryStore,
    incident: &Incident,
    evidence: Evidence,
    at: Timestamp,
) {
    let signal = Signal::firing(
        SourceKind::Agent,
        host_id(),
        CheckKey::parse("docker.container").unwrap(),
        Some(TargetKey::parse("nginx").unwrap()),
        Severity::Warning,
        "nginx is restarting".to_owned(),
        at,
    )
    .with_evidence(evidence);
    SignalRepo::insert(store, &signal, Some(incident.id()))
        .await
        .unwrap();
}

pub async fn world(setup: Setup) -> World {
    let store = Arc::new(match setup.store_delay {
        Some(delay) => InMemoryStore::new().with_delay(delay),
        None => InMemoryStore::new(),
    });
    world_over(setup, store).await
}

/// A world over `store`, so a test can wrap the store to inject failures into the repositories
/// the services read.
pub async fn world_over(setup: Setup, store: Arc<InMemoryStore>) -> World {
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: format!("output of {}", call.tool().name()),
            truncated: false,
        })
    });
    let incident = incident_with(&store, setup.evidence).await;
    // Triage binds a v2 plan to the host's enrolled key, so the world has one.
    let mut registered = domovoi_testkit::build::host("nas").unwrap();
    registered.id = host_id();
    HostRepo::upsert_host(&*store, &registered).await.unwrap();
    HostRepo::put_agent(
        &*store,
        &AgentIdentity {
            agent: link.agent_id(host_id()),
            host: host_id(),
            public_key: [7; 32],
            version: "0.1.0".to_owned(),
            enrolled_at: T0,
            revoked_at: None,
        },
    )
    .await
    .unwrap();
    // The world's own incident stands for one a test drives by hand: it counts as already
    // triaged, so the service's start-up pass leaves it alone and the test decides when
    // triage runs. The entry has no detail, so nothing reads it as a report, and it is
    // written through the repository, so no event is published for it.
    let handled = IncidentTimelineEntry {
        incident: incident.id(),
        at: T0,
        kind: "triage.cached".to_owned(),
        summary: "the incident this world was built around is triaged by its test".to_owned(),
        detail: serde_json::Value::Null,
    };
    domovoi_core::ports::IncidentRepo::append_event(&*store, &handled)
        .await
        .unwrap();
    let (bus, events) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let redactor = Arc::new(PatternRedactor::new([SecretString::from(
        SECRET.to_owned(),
    )]));
    let (manager, manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        uow: store.clone(),
        versions: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: redactor.clone(),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let primary = Arc::new(if setup.cli {
        ScriptedLlm::new().without_tools()
    } else {
        ScriptedLlm::new()
    });
    let secondary = Arc::new(ScriptedLlm::new());
    let third = Arc::new(ScriptedLlm::new());
    let primary_dyn: Arc<dyn LlmProvider> = match setup.provider.clone() {
        Some(provider) => provider,
        None => primary.clone(),
    };
    let secondary_dyn: Arc<dyn LlmProvider> = secondary.clone();
    let third_dyn: Arc<dyn LlmProvider> = third.clone();
    let fallbacks: Vec<Arc<dyn LlmProvider>> = match (setup.secondary, setup.third) {
        (false, false) => Vec::new(),
        (true, false) => vec![secondary_dyn],
        (false, true) => vec![third_dyn],
        (true, true) => vec![secondary_dyn, third_dyn],
    };
    let capabilities = Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
        link.clone(),
    ));
    // The triage and operation services stop on their own token, so a restart can end them.
    let services = shutdown.child_token();
    let experience = ExperienceSource::over(store.clone(), redactor.clone(), clock.clone());
    let mut extra_context = setup.extra_context.clone();
    if setup.with_experience {
        extra_context.push(Arc::new(experience.clone()));
    }
    let deps = TriageDeps {
        primary: primary_dyn.clone(),
        fallbacks: fallbacks.clone(),
        evidence: domovoi_engine::Evidence::new(
            evidence_caps(link.clone()),
            clock.clone(),
            capabilities.clone(),
        ),
        catalog: domovoi_engine::HostCatalog::new(observer_of(link.clone())),
        capabilities: capabilities.clone(),
        resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
        incidents: store.clone(),
        acks: store.clone(),
        signals: setup.signals.clone().unwrap_or_else(|| store.clone()),
        notes: store.clone(),
        hosts: store.clone(),
        cache: store.clone(),
        usage: setup.usage.clone().unwrap_or_else(|| store.clone()),
        admission: setup.admission.clone().unwrap_or_else(|| store.clone()),
        advisories: store.clone(),
        redactor: redactor.clone(),
        clock: clock.clone(),
        manager: manager.clone(),
        bus: bus.clone(),
        shutdown: services.clone(),
        experience: experience.clone(),
        extra_context,
    };
    let usage = deps.usage.clone();
    let admission = deps.admission.clone();
    let settings = setup.settings;
    let (triage, _triage_task) = TriageService::spawn(deps, settings);
    let ops = spawn_ops(&link, &store, &clock, &redactor, &services, triage.clone());
    // Give the spawned service the scheduler before handing the world over: its start-up
    // pass reads the store once, and a world whose incidents are created afterwards must
    // not have them swept up by a pass that runs in the middle of the test. The in-memory
    // store answers every read without waiting, so one poll finishes the pass.
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    World {
        manager_task,
        store,
        clock,
        link,
        capabilities,
        primary,
        secondary,
        third,
        manager,
        triage,
        events,
        bus,
        incident,
        shutdown,
        services,
        ops,
        primary_dyn,
        fallbacks,
        settings,
        redactor,
        usage,
        admission,
    }
}

/// An operation service that investigates through `triage`, over the world's fakes.
fn spawn_ops(
    link: &Arc<FakeAgentLink>,
    store: &Arc<InMemoryStore>,
    clock: &Arc<FakeClock>,
    redactor: &Arc<PatternRedactor>,
    shutdown: &CancellationToken,
    triage: TriageHandle,
) -> OperationHandle {
    let ports = action_caps(link.clone());
    let registry = Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
        link.clone(),
    ));
    let (handle, _task) = OperationService::spawn(
        OperationDeps {
            bus: tokio::sync::broadcast::channel(16).0,
            operations: store.clone(),
            experience: store.clone(),
            uow: store.clone(),
            effects: store.clone(),
            effect_read: store.clone(),
            advisories: store.clone(),
            incidents: store.clone(),
            hosts: store.clone(),
            evidence: domovoi_engine::Evidence::new(
                evidence_caps(link.clone()),
                clock.clone(),
                registry.clone(),
            ),
            registry,
            resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
            observer: observer_of(link.clone()),
            journal: ports.journal,
            handoff: ports.handoff,
            rerun: ports.rerun,
            redactor: redactor.clone(),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
            investigate: Some(InvestigateDeps {
                triage,
                settings: InvestigateSettings::default(),
            }),
            autonomy: None,
            contain: None,
        },
        OperationSettings::default(),
    );
    handle
}

impl World {
    /// A second triage and operation service over the same store and clock, as a hub restart
    /// builds: the first services stop, so no two effect runners share the store.
    pub fn restart_triage(&self) -> OperationHandle {
        self.services.cancel();
        let services = self.shutdown.child_token();
        let deps = TriageDeps {
            primary: self.primary_dyn.clone(),
            fallbacks: self.fallbacks.clone(),
            evidence: domovoi_engine::Evidence::new(
                evidence_caps(self.link.clone()),
                self.clock.clone(),
                Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
                    self.link.clone(),
                )),
            ),
            catalog: domovoi_engine::HostCatalog::new(observer_of(self.link.clone())),
            capabilities: Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
                self.link.clone(),
            )),
            resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
            incidents: self.store.clone(),
            acks: self.store.clone(),
            signals: self.store.clone(),
            notes: self.store.clone(),
            hosts: self.store.clone(),
            cache: self.store.clone(),
            usage: self.usage.clone(),
            admission: self.admission.clone(),
            advisories: self.store.clone(),
            redactor: self.redactor.clone(),
            clock: self.clock.clone(),
            manager: self.manager.clone(),
            bus: self.bus.clone(),
            shutdown: services.clone(),
            experience: ExperienceSource::over(
                self.store.clone(),
                self.redactor.clone(),
                self.clock.clone(),
            ),

            extra_context: Vec::new(),
        };
        let (triage, _task) = TriageService::spawn(deps, self.settings);
        spawn_ops(
            &self.link,
            &self.store,
            &self.clock,
            &self.redactor,
            &services,
            triage,
        )
    }

    /// Adds a newer signal with `evidence`, one second later.
    pub async fn signal(&self, evidence: Evidence) {
        self.clock.advance(jiff::SignedDuration::from_secs(1));
        add_signal(&self.store, &self.incident, evidence, self.now()).await;
    }

    pub fn now(&self) -> Timestamp {
        Clock::now(&*self.clock)
    }

    /// Every event published so far.
    pub fn drain(&mut self) -> Vec<DomainEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.push(event);
        }
        out
    }

    /// The plain chat tokens of the last `TriageCompleted` event.
    pub fn tokens(&mut self) -> BTreeMap<domovoi_core::ExecutionId, String> {
        self.drain()
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::TriageCompleted { tokens, .. } => Some(tokens),
                _ => None,
            })
            .next_back()
            .unwrap_or_default()
    }

    pub async fn note(&self, title: &str, body: &str) {
        let note = Note {
            id: NoteId::new(),
            host: Some(host_id()),
            title: title.to_owned(),
            body_markdown: body.to_owned(),
            tags: Vec::new(),
            updated_at: T0,
        };
        NoteRepo::upsert(&*self.store, &note).await.unwrap();
    }
}

/// A valid report as JSON, with `proposals`.
pub fn report_json(verdict: &str, proposals: Value) -> String {
    json!({
        "headline": "nginx keeps restarting",
        "summary": "The container exits a few seconds after start.",
        "cause": "A bad config file, fairly sure.",
        "evidence": [{"label": "exit code", "value": "1", "source": "docker_logs"}],
        "verdict": verdict,
        "next_step": "Fix the config file and restart the container.",
        "proposals": proposals,
        "details": ""
    })
    .to_string()
}

pub fn report() -> String {
    report_json("ACTION_NEEDED", json!([]))
}

/// A model turn that calls tools.
pub fn calls(list: &[(&str, &str, Value)]) -> LlmResponse {
    LlmResponse {
        parts: list
            .iter()
            .map(|(id, name, input)| Part::ToolCall {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                input: input.clone(),
            })
            .collect(),
        stop: StopReason::ToolUse,
        usage: Usage {
            input_tokens: 100,
            output_tokens: 10,
            cost_micros: Some(50),
            source: domovoi_core::ports::UsageSource::Reported,
        },
    }
}

/// A model turn that reads through `request_evidence`: each `(id, read tool, args)` becomes
/// one `request_evidence` call naming that read tool.
pub fn read_calls(list: &[(&str, &str, Value)]) -> LlmResponse {
    let wrapped: Vec<(&str, &str, Value)> = list
        .iter()
        .map(|(id, tool, args)| (*id, "request_evidence", json!({"tool": tool, "args": args})))
        .collect();
    calls(&wrapped)
}

/// A model turn with one `propose_plan` call holding `steps`.
pub fn plan_call(id: &str, steps: Value) -> LlmResponse {
    calls(&[(id, "propose_plan", json!({ "steps": steps }))])
}

/// A model turn with one `finish` call holding `report` (a report without proposals).
pub fn finish_call(id: &str, report: Value) -> LlmResponse {
    calls(&[(id, "finish", report)])
}

/// A model turn with one `ask_user` call.
pub fn ask_call(id: &str, question: &str) -> LlmResponse {
    calls(&[(id, "ask_user", json!({ "question": question }))])
}

/// The tool results in the last message of `req`.
pub fn results(req: &domovoi_core::ports::LlmRequest) -> Vec<(String, bool)> {
    req.messages
        .last()
        .map(|m| {
            m.parts
                .iter()
                .filter_map(|p| match p {
                    Part::ToolResult {
                        content, is_error, ..
                    } => Some((content.clone(), *is_error)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every text the model was shown in `req`: system, messages and tool results.
pub fn seen(req: &domovoi_core::ports::LlmRequest) -> String {
    format!("{req:?}")
}

/// `restart_container` with one `container` parameter.
pub fn restart_skill() -> SkillSummary {
    let mut manifest = manifest("restart_container", Risk::Write).unwrap();
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").unwrap(),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}
