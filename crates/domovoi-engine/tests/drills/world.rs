//! Runs one drill through the real incident manager and triage service, with fakes for the
//! store, the clock and the agent link, and either the drill's scripted model turns or a
//! real provider.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionFilter, ActionRepo, IncidentRepo, LlmError, LlmProvider, LlmRequest, LlmResponse, Page,
    Part, Presence, SignalRepo, StopReason, ToolOutput, Usage,
};
use domovoi_core::{
    ActionId, ActionRequest, CheckKey, Evidence, HostId, HostName, Incident, Severity, Signal,
    SkillManifest, SkillSummary, SourceKind, TargetKey, TriageOutcome,
};
use domovoi_engine::PatternRedactor;
use domovoi_engine::triage::{TriageDeps, TriageService, TriageSettings};
use domovoi_engine::{DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps};
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::LinkRequest;
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore, ScriptedLlm};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::format::{Drill, IncidentSpec, Turn};

pub const HOST: &str = "nas";

pub fn host_id() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00d1))
}

/// Which model answers.
pub enum Model {
    /// The drill's own scripted turns.
    Scripted,
    /// A real provider.
    Live(Arc<dyn LlmProvider>),
}

/// Everything a drill run produced, for scoring and probing.
pub struct Outcome {
    pub outcome: TriageOutcome,
    pub incident: Incident,
    pub actions: Vec<ActionRequest>,
    pub tokens: BTreeMap<ActionId, String>,
    pub requests: Vec<LlmRequest>,
    pub link: Vec<LinkRequest>,
    pub latest: Option<Signal>,
}

/// Records every request on the way to another provider.
struct Recording {
    inner: Arc<dyn LlmProvider>,
    requests: Mutex<Vec<LlmRequest>>,
}

#[async_trait]
impl LlmProvider for Recording {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn supports_tools(&self) -> bool {
        self.inner.supports_tools()
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.requests.lock().unwrap().push(req.clone());
        self.inner.complete(req).await
    }
}

/// The starter skill `name`, read from `examples/skills/<name>/skill.toml`.
pub fn starter_skill(name: &str) -> SkillSummary {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/skills")
        .join(name)
        .join("skill.toml");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let manifest: SkillManifest = toml::from_str(&text).unwrap();
    manifest.validate().unwrap();
    manifest.summary()
}

fn turn(turn: &Turn, index: usize) -> LlmResponse {
    match turn {
        Turn::Calls { calls } => LlmResponse {
            parts: calls
                .iter()
                .enumerate()
                .map(|(n, call)| Part::ToolCall {
                    id: format!("t{index}c{n}"),
                    name: call.tool.clone(),
                    input: call.input.clone(),
                })
                .collect(),
            stop: StopReason::ToolUse,
            usage: Usage {
                input_tokens: 500,
                output_tokens: 50,
                cost_micros: None,
            },
        },
        Turn::Report { report } => LlmResponse {
            parts: vec![Part::Text(report.to_string())],
            stop: StopReason::EndTurn,
            usage: Usage {
                input_tokens: 800,
                output_tokens: 300,
                cost_micros: None,
            },
        },
    }
}

async fn signal(
    store: &InMemoryStore,
    incident: &Incident,
    spec_check: &str,
    target: Option<&str>,
    summary: &str,
    evidence: Evidence,
    at: Timestamp,
) -> Signal {
    let signal = Signal::firing(
        SourceKind::Agent,
        host_id(),
        CheckKey::parse(spec_check).unwrap(),
        target.map(|t| TargetKey::parse(t).unwrap()),
        Severity::Warning,
        summary.to_owned(),
        at,
    )
    .with_evidence(evidence);
    SignalRepo::insert(store, &signal, Some(incident.id()))
        .await
        .unwrap();
    signal
}

async fn open(store: &InMemoryStore, spec: &IncidentSpec, at: Timestamp) -> (Incident, Signal) {
    let incident = open_incident(host_id(), &spec.check, spec.target.as_deref()).unwrap();
    IncidentRepo::insert(store, &incident).await.unwrap();
    let first = signal(
        store,
        &incident,
        &spec.check,
        spec.target.as_deref(),
        &spec.summary,
        spec.evidence.clone(),
        at,
    )
    .await;
    (incident, first)
}

/// Runs `drill` once with `model`.
pub async fn run(drill: &Drill, model: Model) -> Outcome {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    );
    link.set_skills(
        host_id(),
        drill
            .setup
            .skills
            .iter()
            .map(|s| starter_skill(s))
            .collect(),
    );
    let responses: Vec<(String, Option<String>, String)> = drill
        .agent_responses
        .iter()
        .map(|r| (r.tool.clone(), r.matches.clone(), r.output.clone()))
        .collect();
    link.on_tool(move |_, call| {
        let name = call.tool().name();
        let text = format!("{call:?}");
        let output = responses
            .iter()
            .find(|(tool, matches, _)| {
                tool == name && matches.as_ref().is_none_or(|m| text.contains(m.as_str()))
            })
            .map_or_else(
                || "no data for this call".to_owned(),
                |(_, _, out)| out.clone(),
            );
        Ok(ToolOutput {
            text: output,
            truncated: false,
        })
    });

    for (n, decoy) in drill.setup.decoys.iter().enumerate() {
        let at = T0 - SignedDuration::from_mins(30 + i64::try_from(n).unwrap());
        open(&store, decoy, at).await;
    }
    let (incident, first) = open(&store, &drill.setup.incident, T0).await;
    let mut latest = first;
    for later in &drill.setup.signals {
        clock.advance(SignedDuration::from_secs(
            i64::try_from(later.after_secs).unwrap(),
        ));
        latest = signal(
            &store,
            &incident,
            &drill.setup.incident.check,
            drill.setup.incident.target.as_deref(),
            &later.summary,
            later.evidence.clone(),
            domovoi_core::ports::Clock::now(&*clock),
        )
        .await;
    }

    let (bus, mut events) = broadcast::channel::<DomainEvent>(256);
    let shutdown = CancellationToken::new();
    let redactor = Arc::new(PatternRedactor::new([SecretString::from(
        drill.setup.secret.clone(),
    )]));
    let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
        incidents: store.clone(),
        signals: store.clone(),
        acks: store.clone(),
        clock: clock.clone(),
        redactor: redactor.clone(),
        policy: EnginePolicy::default(),
        bus: bus.clone(),
        shutdown: shutdown.clone(),
    });
    let inner: Arc<dyn LlmProvider> = match model {
        Model::Scripted => {
            let scripted = ScriptedLlm::new();
            for (index, step) in drill.model.iter().enumerate() {
                scripted.push_ok(turn(step, index));
            }
            Arc::new(scripted)
        }
        Model::Live(provider) => provider,
    };
    let recording = Arc::new(Recording {
        inner,
        requests: Mutex::new(Vec::new()),
    });
    let primary: Arc<dyn LlmProvider> = recording.clone();
    let deps = TriageDeps {
        primary,
        secondary: None,
        link: link.clone(),
        incidents: store.clone(),
        signals: store.clone(),
        notes: store.clone(),
        actions: store.clone(),
        audit: store.clone(),
        cache: store.clone(),
        usage: store.clone(),
        advisories: store.clone(),
        redactor,
        clock: clock.clone(),
        manager,
        bus,
        shutdown: shutdown.clone(),
    };
    let (triage, _triage_task) = TriageService::spawn(deps, TriageSettings::default());
    let outcome = triage.triage(&incident).await;

    let mut tokens = BTreeMap::new();
    while let Ok(event) = events.try_recv() {
        if let EventKind::TriageCompleted { tokens: minted, .. } = event.kind {
            tokens = minted;
        }
    }
    let actions = ActionRepo::list(&*store, &ActionFilter::default(), &Page::first(100))
        .await
        .unwrap()
        .items;
    let requests = recording.requests.lock().unwrap().clone();
    shutdown.cancel();
    Outcome {
        outcome,
        incident,
        actions,
        tokens,
        requests,
        link: link.requests(),
        latest: Some(latest),
    }
}

/// The drill host's name.
pub fn host_name() -> HostName {
    HostName::parse(HOST).unwrap()
}
