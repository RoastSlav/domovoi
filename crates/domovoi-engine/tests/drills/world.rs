//! Runs one drill through the real incident manager and triage service, with fakes for the
//! store, the clock and the agent link, and either the drill's scripted model turns or a
//! real provider.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionFilter, ActionRepo, HostRepo, IncidentRepo, LlmError, LlmProvider, LlmRequest,
    LlmResponse, OperationRepo, Page, Part, Presence, SignalRepo, StopReason, ToolOutput, Usage,
};
use domovoi_core::{
    ActionId, ActionRequest, AgentId, AgentIdentity, CheckKey, Evidence, HostId, HostName,
    Incident, Severity, Signal, SkillManifest, SkillSummary, SourceKind, TargetKey, TriageOutcome,
};
use domovoi_engine::PatternRedactor;
use domovoi_engine::operations::{
    InvestigateDeps, InvestigateSettings, OperationDeps, OperationService, OperationSettings,
};
use domovoi_engine::triage::{TriageDeps, TriageService, TriageSettings};
use domovoi_engine::{DomainEvent, EnginePolicy, EventKind, IncidentManager, ManagerDeps};
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::LinkRequest;
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore, ScriptedLlm};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::format::{Drill, IncidentSpec, Turn};
use crate::proposal_view::{ProposalView, from_snapshot};

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
    /// The legacy actions; none, since investigations store Execution steps.
    pub actions: Vec<ActionRequest>,
    /// What was proposed, from the actions or from the Execution steps.
    pub proposals: Vec<ProposalView>,
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
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

/// How a scripted report reaches the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carrier {
    /// As a JSON text answer.
    Text,
    /// As a `propose_plan` call (when it has proposals) and a `finish` call in one response.
    Meta,
}

fn usage(input: u32, output: u32) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cost_micros: None,
        source: domovoi_core::ports::UsageSource::Reported,
    }
}

/// The `propose_plan` steps of a scripted report's proposals, as skill contracts.
fn plan_steps(proposals: &[serde_json::Value]) -> Vec<serde_json::Value> {
    proposals
        .iter()
        .map(|proposal| {
            let skill = proposal["skill"].as_str().unwrap_or_default();
            let mut step = serde_json::json!({
                "contract": format!("skill.{skill}"),
                "reason": proposal["reason"].clone(),
            });
            if let Some(args) = proposal.get("args") {
                step["args"] = args.clone();
            }
            step
        })
        .collect()
}

fn turn(turn: &Turn, index: usize, carrier: Carrier) -> LlmResponse {
    match turn {
        Turn::Calls { calls } => LlmResponse {
            parts: calls
                .iter()
                .enumerate()
                .map(|(n, call)| Part::ToolCall {
                    id: format!("t{index}c{n}"),
                    name: "request_evidence".to_owned(),
                    input: serde_json::json!({"tool": call.tool, "args": call.input}),
                })
                .collect(),
            stop: StopReason::ToolUse,
            usage: usage(500, 50),
        },
        Turn::Report { report } if carrier == Carrier::Meta => {
            let mut finish = report.clone();
            let proposals = finish
                .as_object_mut()
                .and_then(|object| object.remove("proposals"))
                .and_then(|value| value.as_array().cloned())
                .unwrap_or_default();
            let mut parts = Vec::new();
            if !proposals.is_empty() {
                parts.push(Part::ToolCall {
                    id: format!("t{index}plan"),
                    name: "propose_plan".to_owned(),
                    input: serde_json::json!({ "steps": plan_steps(&proposals) }),
                });
            }
            parts.push(Part::ToolCall {
                id: format!("t{index}finish"),
                name: "finish".to_owned(),
                input: finish,
            });
            LlmResponse {
                parts,
                stop: StopReason::ToolUse,
                usage: usage(800, 300),
            }
        }
        Turn::Report { report } => LlmResponse {
            parts: vec![Part::Text(report.to_string())],
            stop: StopReason::EndTurn,
            usage: usage(800, 300),
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
    // The drill drives triage itself, so the service's start-up pass must leave every
    // incident it seeds alone: the entry marks it as already triaged, and its empty detail
    // is never read as a report.
    let handled = domovoi_core::ports::IncidentTimelineEntry {
        incident: incident.id(),
        at,
        kind: "triage.cached".to_owned(),
        summary: "the drill triages its own incident".to_owned(),
        detail: serde_json::Value::Null,
    };
    IncidentRepo::append_event(store, &handled).await.unwrap();
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

/// Runs `drill` once with `model`, investigated as an operation; a scripted report is a
/// text answer.
pub async fn run(drill: &Drill, model: Model) -> Outcome {
    run_with(drill, model, Carrier::Text).await
}

/// Runs `drill` once with `model`, its scripted report delivered through `carrier`.
pub async fn run_with(drill: &Drill, model: Model, carrier: Carrier) -> Outcome {
    let store = Arc::new(InMemoryStore::new());
    let mut registered = domovoi_testkit::build::host(HOST).unwrap();
    registered.id = host_id();
    HostRepo::upsert_host(&*store, &registered).await.unwrap();
    let agent_id = AgentId::new();
    HostRepo::put_agent(
        &*store,
        &AgentIdentity {
            agent: agent_id,
            host: host_id(),
            public_key: [7; 32],
            version: "0.1.0".to_owned(),
            enrolled_at: T0,
            revoked_at: None,
        },
    )
    .await
    .unwrap();
    let clock = Arc::new(FakeClock::new(T0));
    let link = Arc::new(FakeAgentLink::new());
    link.set_agent(host_id(), agent_id);
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
        uow: store.clone(),
        versions: store.clone(),
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
                scripted.push_ok(turn(step, index, carrier));
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
        fallbacks: Vec::new(),
        evidence: domovoi_engine::Evidence::new(
            evidence_caps(link.clone()),
            clock.clone(),
            Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
                link.clone(),
            )),
        ),
        catalog: domovoi_engine::HostCatalog::new(observer_of(link.clone())),
        capabilities: Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
            link.clone(),
        )),
        resolver: Arc::new(domovoi_testkit::FakeTargetResolver::new()),
        incidents: store.clone(),
        signals: store.clone(),
        notes: store.clone(),
        hosts: store.clone(),
        actions: store.clone(),
        cache: store.clone(),
        usage: store.clone(),
        admission: store.clone(),
        advisories: store.clone(),
        redactor: redactor.clone(),
        clock: clock.clone(),
        manager,
        bus,
        shutdown: shutdown.clone(),
    };
    let (triage, _triage_task) = TriageService::spawn(deps, TriageSettings::default());
    let outcome = {
        let ports = action_caps(link.clone());
        let registry = Arc::new(domovoi_testkit::FakeCapabilityRegistry::following(
            link.clone(),
        ));
        let (ops, _ops_task) = OperationService::spawn(
            OperationDeps {
                bus: tokio::sync::broadcast::channel(16).0,
                operations: store.clone(),
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
                    triage: triage.clone(),
                    settings: InvestigateSettings::default(),
                }),
            },
            OperationSettings::default(),
        );
        let asked = incident.clone();
        let run = tokio::spawn(async move { ops.investigate_and_wait(&asked, false).await });
        // The effect runner works against the fake clock. A scripted drill needs no time
        // to pass, so the clock moves only when the investigation has stayed unfinished
        // for a long while, and never far enough to reach the run's deadline on a
        // loaded machine.
        for round in 0..60_000_u32 {
            for _ in 0..100 {
                tokio::task::yield_now().await;
            }
            if run.is_finished() {
                break;
            }
            if round % 600 == 599 {
                clock.advance(SignedDuration::from_secs(1));
            }
        }
        assert!(run.is_finished(), "the investigation never gave an outcome");
        run.await
            .expect("the investigation task finished")
            .unwrap_or_else(|err| panic!("the investigation gave no outcome: {err}"))
    };

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
    let proposals = {
        let skills: Vec<SkillSummary> = drill
            .setup
            .skills
            .iter()
            .map(|name| starter_skill(name))
            .collect();
        let mut views = Vec::new();
        for operation in OperationRepo::for_incident(&*store, incident.id())
            .await
            .unwrap()
        {
            let snapshot = OperationRepo::load(&*store, operation.id)
                .await
                .unwrap()
                .unwrap();
            views.extend(from_snapshot(&snapshot, &skills));
        }
        views
    };
    shutdown.cancel();
    Outcome {
        outcome,
        incident,
        actions,
        proposals,
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
