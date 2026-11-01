//! A running incident manager and triage service over fakes, with one open incident.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionFilter, ActionRepo, Clock, LlmProvider, LlmResponse, NoteRepo, Page, Part, Presence,
    SignalRepo, StopReason, ToolOutput, Usage,
};
use domovoi_core::{
    ActionRequest, CheckKey, Evidence, EvidenceValue, HostId, Incident, Note, NoteId, ParamKind,
    ParamName, ParamSpec, Risk, Severity, Signal, SkillSummary, SourceKind, TargetKey,
    TriageOutcome,
};
use domovoi_engine::triage::{TriageDeps, TriageHandle, TriageService, TriageSettings};
use domovoi_engine::{DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps};
use domovoi_engine::{ManagerHandle, PatternRedactor};
use domovoi_testkit::build::{T0, manifest, open_incident};
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
    pub cli: bool,
    pub evidence: Evidence,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            settings: TriageSettings::default(),
            secondary: false,
            cli: false,
            evidence: disk(92.4),
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
    pub store: Arc<InMemoryStore>,
    pub clock: Arc<FakeClock>,
    pub link: Arc<FakeAgentLink>,
    pub primary: Arc<ScriptedLlm>,
    pub secondary: Arc<ScriptedLlm>,
    pub manager: ManagerHandle,
    pub triage: TriageHandle,
    pub events: broadcast::Receiver<DomainEvent>,
    pub bus: broadcast::Sender<DomainEvent>,
    pub incident: Incident,
    pub shutdown: CancellationToken,
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
    link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: format!("output of {}", call.tool().name()),
            truncated: false,
        })
    });
    let incident = incident_with(&store, setup.evidence).await;
    let (bus, events) = broadcast::channel(256);
    let shutdown = CancellationToken::new();
    let (manager, _task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
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
    let primary_dyn: Arc<dyn LlmProvider> = primary.clone();
    let secondary_dyn: Arc<dyn LlmProvider> = secondary.clone();
    let deps = TriageDeps {
        primary: primary_dyn,
        secondary: setup.secondary.then_some(secondary_dyn),
        link: link.clone(),
        incidents: store.clone(),
        signals: store.clone(),
        notes: store.clone(),
        actions: store.clone(),
        cache: store.clone(),
        usage: store.clone(),
        advisories: store.clone(),
        redactor: Arc::new(PatternRedactor::new([SecretString::from(
            SECRET.to_owned(),
        )])),
        clock: clock.clone(),
        manager: manager.clone(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    };
    let (triage, _task) = TriageService::spawn(deps, setup.settings);
    World {
        store,
        clock,
        link,
        primary,
        secondary,
        manager,
        triage,
        events,
        bus,
        incident,
        shutdown,
    }
}

impl World {
    /// Adds a newer signal with `evidence`, one second later.
    pub async fn signal(&self, evidence: Evidence) {
        self.clock.advance(jiff::SignedDuration::from_secs(1));
        add_signal(&self.store, &self.incident, evidence, self.now()).await;
    }

    pub fn now(&self) -> Timestamp {
        Clock::now(&*self.clock)
    }

    pub async fn run(&self) -> TriageOutcome {
        self.triage.triage(&self.incident).await
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
    pub fn tokens(&mut self) -> BTreeMap<domovoi_core::ActionId, String> {
        self.drain()
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::TriageCompleted { tokens, .. } => Some(tokens),
                _ => None,
            })
            .next_back()
            .unwrap_or_default()
    }

    pub async fn actions(&self) -> Vec<ActionRequest> {
        ActionRepo::list(&*self.store, &ActionFilter::default(), &Page::first(100))
            .await
            .unwrap()
            .items
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
        },
    }
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
    manifest.summary()
}
