//! A running incident manager, triage service (queue off), action service and operation
//! service with investigations, over one in-memory store and one fake clock.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateKind, ClaimBatch, EffectKind, EffectQueue, HostRepo, LlmProvider, LlmResponse, Part,
    Presence, SignalRepo, StopReason, StoreError, ToolOutput, Usage, UsageSource,
};
use domovoi_core::{
    AgentIdentity, Evidence, EvidenceValue, Incident, JournalEpoch, Severity, Signal, SourceKind,
    UserId,
};
use domovoi_engine::operations::{
    InvestigateDeps, OperationDeps, OperationHandle, OperationService, OperationSettings,
};
use domovoi_engine::triage::{TriageDeps, TriageService, TriageSettings};
use domovoi_engine::{
    ActionDeps, ActionService, ActionSettings, DomainEvent, EnginePolicy, IncidentManager,
    ManagerDeps, PatternRedactor,
};
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakeTargetResolver, InMemoryStore,
    ScriptedLlm,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::harness::{host_id, restart_skill};

/// A value registered with the redactor that must never reach the model.
pub const SECRET: &str = "hunter2-correct-horse-battery";

/// An effect queue that never hands out the kinds a test holds back.
pub struct KindGate {
    inner: Arc<InMemoryStore>,
    held: Mutex<Vec<EffectKind>>,
}

impl KindGate {
    /// A gate over `inner` that holds back `held`.
    pub fn new(inner: Arc<InMemoryStore>, held: &[EffectKind]) -> Arc<Self> {
        Arc::new(Self {
            inner,
            held: Mutex::new(held.to_vec()),
        })
    }

    /// Lets every kind through from now on.
    pub fn open(&self) {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[async_trait]
impl EffectQueue for KindGate {
    async fn claim_due(
        &self,
        runner: &str,
        kinds: &[EffectKind],
        aggregates: &[AggregateKind],
        now: Timestamp,
        lease: SignedDuration,
        limit: u16,
    ) -> Result<ClaimBatch, StoreError> {
        let held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let allowed: Vec<EffectKind> = kinds
            .iter()
            .copied()
            .filter(|kind| !held.contains(kind))
            .collect();
        if allowed.is_empty() {
            return Ok(ClaimBatch::default());
        }
        self.inner
            .claim_due(runner, &allowed, aggregates, now, lease, limit)
            .await
    }

    async fn revert_claimed_at_start(
        &self,
        aggregates: &[AggregateKind],
    ) -> Result<u64, StoreError> {
        self.inner.revert_claimed_at_start(aggregates).await
    }
}

/// How an investigation world is built.
pub struct InvSetup {
    /// Triage limits; `run_queue` is forced off.
    pub triage: TriageSettings,
    /// Operation limits.
    pub ops: OperationSettings,
    /// Effect kinds the runner does not claim until `gate.open()`.
    pub held: Vec<EffectKind>,
    /// The evidence of the incident's first signal.
    pub evidence: Evidence,
}

impl Default for InvSetup {
    fn default() -> Self {
        Self {
            triage: TriageSettings::default(),
            ops: OperationSettings::default(),
            held: Vec::new(),
            evidence: disk(92.4),
        }
    }
}

/// Evidence with one disk reading.
pub fn disk(used: f64) -> Evidence {
    let mut evidence = Evidence::new();
    evidence.insert("used_pct", EvidenceValue::Float(used));
    evidence.insert(
        "observed",
        EvidenceValue::Text("2026-10-31T11:52:00Z".to_owned()),
    );
    evidence
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

/// A report with no proposals.
pub fn report() -> String {
    report_json("ACTION_NEEDED", json!([]))
}

/// A report proposing to restart `container`.
pub fn restart_report(container: &str) -> String {
    report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": container}, "reason": "It is stuck."}]),
    )
}

/// A final answer that reports `input` and `output` tokens.
pub fn answer_with_usage(text: String, input: u32, output: u32) -> LlmResponse {
    LlmResponse {
        parts: vec![Part::Text(text)],
        stop: StopReason::EndTurn,
        usage: Usage {
            input_tokens: input,
            output_tokens: output,
            cost_micros: Some(40),
            source: UsageSource::Reported,
        },
    }
}

/// The running world.
pub struct Inv {
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    /// The registry triage reads; independent of the link so a test can offer a skill the
    /// agent does not.
    pub caps: Arc<FakeCapabilityRegistry>,
    pub llm: Arc<ScriptedLlm>,
    pub bus: broadcast::Sender<DomainEvent>,
    pub incident: Incident,
    pub gate: Arc<KindGate>,
    pub user: UserId,
    pub ops: OperationHandle,
    pub settings: OperationSettings,
    pub(crate) deps: OperationDeps,
    pub(crate) ops_shutdown: CancellationToken,
    pub(crate) ops_task: Option<tokio::task::JoinHandle<()>>,
    shutdown: CancellationToken,
    pub(crate) events: Mutex<broadcast::Receiver<DomainEvent>>,
    pub(crate) seen: Mutex<Vec<DomainEvent>>,
}

impl Drop for Inv {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.ops_shutdown.cancel();
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Adds a firing signal with `evidence` observed at `at` to `incident`.
pub async fn add_signal(
    store: &InMemoryStore,
    incident: &Incident,
    evidence: Evidence,
    at: Timestamp,
) {
    let signal = Signal::firing(
        SourceKind::Agent,
        host_id(),
        incident.check().clone(),
        incident.target().cloned(),
        Severity::Warning,
        "nginx is restarting".to_owned(),
        at,
    )
    .with_evidence(evidence);
    SignalRepo::insert(store, &signal, Some(incident.id()))
        .await
        .unwrap();
}

/// Builds the world, with the investigation service already past its start-up pass.
pub async fn inv(setup: InvSetup) -> Inv {
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
    link.set_skills(host_id(), vec![restart_skill()]);
    link.set_journal_epoch(host_id(), JournalEpoch::from_uuid(uuid::Uuid::now_v7()));
    link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: format!("output of {}", call.tool().name()),
            truncated: false,
        })
    });
    link.on_skill(|_, _| Ok(crate::drive::ok_output("restarted nginx")));
    link.on_check(|_, _, _| Ok(crate::drive::resolved()));
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
    let (bus, events) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let redactor = Arc::new(PatternRedactor::new([SecretString::from(
        SECRET.to_owned(),
    )]));
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
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
    let llm = Arc::new(ScriptedLlm::new());
    let primary: Arc<dyn LlmProvider> = llm.clone();
    let action_ports = action_caps(link.clone());
    let (actions_service, _actions_task) = ActionService::spawn(
        ActionDeps {
            actions: store.clone(),
            incidents: store.clone(),
            audit: store.clone(),
            uow: store.clone(),
            effects: store.clone(),
            advisories: store.clone(),
            probe: action_ports.probe,
            observer: action_ports.observer,
            journal: action_ports.journal,
            capabilities: Arc::new(FakeCapabilityRegistry::following(link.clone())),
            handoff: action_ports.handoff,
            rerun: action_ports.rerun,
            hosts: store.clone(),
            redactor: redactor.clone(),
            clock: clock.clone(),
            manager: manager.clone(),
            bus: bus.clone(),
            shutdown: shutdown.clone(),
        },
        ActionSettings::default(),
    );
    let caps = Arc::new(FakeCapabilityRegistry::empty());
    caps.set_skills(host_id(), vec![restart_skill()]);
    let triage_deps = TriageDeps {
        primary,
        fallbacks: Vec::new(),
        evidence: domovoi_engine::Evidence::new(
            evidence_caps(link.clone()),
            clock.clone(),
            caps.clone(),
        ),
        catalog: domovoi_engine::HostCatalog::new(observer_of(link.clone())),
        capabilities: caps.clone(),
        incidents: store.clone(),
        signals: store.clone(),
        notes: store.clone(),
        hosts: store.clone(),
        actions: store.clone(),
        actions_service,
        cache: store.clone(),
        usage: store.clone(),
        admission: store.clone(),
        advisories: store.clone(),
        redactor: redactor.clone(),
        clock: clock.clone(),
        manager: manager.clone(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    };
    let triage_settings = TriageSettings {
        run_queue: false,
        ..setup.triage
    };
    let (triage, _triage_task) = TriageService::spawn(triage_deps, triage_settings);
    let gate = KindGate::new(store.clone(), &setup.held);
    let ops_shutdown = CancellationToken::new();
    let ops_caps = action_caps(link.clone());
    let registry = Arc::new(FakeCapabilityRegistry::following(link.clone()));
    let deps = OperationDeps {
        bus: tokio::sync::broadcast::channel(16).0,
        operations: store.clone(),
        uow: store.clone(),
        effects: gate.clone(),
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
        resolver: Arc::new(FakeTargetResolver::new()),
        observer: observer_of(link.clone()),
        journal: ops_caps.journal,
        handoff: ops_caps.handoff,
        rerun: ops_caps.rerun,
        redactor: redactor.clone(),
        clock: clock.clone(),
        shutdown: ops_shutdown.clone(),
        investigate: Some(InvestigateDeps {
            triage: triage.clone(),
        }),
    };
    let (ops, ops_task) = OperationService::spawn(deps.clone(), setup.ops.clone());
    let placeholder = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    let mut world = Inv {
        store,
        clock,
        link,
        caps,
        llm,
        bus,
        incident: placeholder,
        gate,
        user: UserId::new(),
        ops,
        settings: setup.ops,
        deps,
        ops_shutdown,
        ops_task: Some(ops_task),
        shutdown,
        events: Mutex::new(events),
        seen: Mutex::new(Vec::new()),
    };
    // The service's start-up pass reads the store once; the incident is created after it so
    // the pass does not take it, and the test decides when the investigation starts.
    world.settle().await;
    world.incident = world.add_incident("nginx", setup.evidence).await;
    world
}

pub use crate::inv_drive::{check, of_kind, output_kind, target, turns};
