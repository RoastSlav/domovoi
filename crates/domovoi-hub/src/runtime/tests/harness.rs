use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use domovoi_config::{HubConfig, MapEnv};
use domovoi_core::ports::{
    AdvisoryRepo, ChatGateway, Clock, IncidentFilter, IncidentRepo, LlmProvider, LlmRequest,
    LlmResponse, NotifyStateRepo, Page, Part, SignalRepo, StopReason, Usage,
};
use domovoi_core::{Advisory, Incident, IncidentId, NotifyState};
use domovoi_testkit::{FakeClock, RecordingGateway, RecordingNotifier, ScriptedLlm};
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use sha2::Sha256;
use tempfile::TempDir;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tower::ServiceExt;

use super::super::{AdapterOverrides, HubDeps, HubRuntime};

pub const SECRET: &str = "runtime-test-secret";
pub const REPORT: &str = r#"{
  "headline": "Backup needs attention",
  "summary": "The nightly backup failed.",
  "cause": "The backup destination rejected the run.",
  "verdict": "ACTION_NEEDED",
  "next_step": "Inspect the backup destination."
}"#;

pub struct Harness {
    pub clock: Arc<FakeClock>,
    pub gateway: Arc<RecordingGateway>,
    pub notifier: Arc<RecordingNotifier>,
    pub runtime: Option<HubRuntime>,
    config: HubConfig,
    primary: Arc<dyn LlmProvider>,
    gateways: Vec<Arc<RecordingGateway>>,
    bus_capacity: usize,
    /// Whether the webhook notifier is configured; without it, only chat cards go out.
    notifier_on: bool,
    tasks: Vec<JoinHandle<()>>,
    _dir: TempDir,
}

impl Harness {
    pub async fn new(extra: &str, start: Timestamp) -> Self {
        let llm = Arc::new(ScriptedLlm::new());
        Self::with_adapters(extra, start, llm, Vec::new(), 4096).await
    }

    pub async fn with_heartbeat(extra: &str, start: Timestamp, url: &str) -> Self {
        let llm = Arc::new(ScriptedLlm::new());
        Self::with_all(
            extra,
            start,
            llm,
            Vec::new(),
            4096,
            MapEnv::new().with("HEARTBEAT_URL", url),
            true,
        )
        .await
    }

    /// A hub with the chat gateway only: no notifier for digests or quiet-hours summaries.
    pub async fn without_notifier(extra: &str, start: Timestamp) -> Self {
        let llm = Arc::new(ScriptedLlm::new());
        Self::with_all(extra, start, llm, Vec::new(), 4096, MapEnv::new(), false).await
    }

    pub async fn with_adapters(
        extra: &str,
        start: Timestamp,
        primary: Arc<dyn LlmProvider>,
        gateways: Vec<Arc<RecordingGateway>>,
        bus_capacity: usize,
    ) -> Self {
        Self::with_all(
            extra,
            start,
            primary,
            gateways,
            bus_capacity,
            MapEnv::new(),
            true,
        )
        .await
    }

    async fn with_all(
        extra: &str,
        start: Timestamp,
        primary: Arc<dyn LlmProvider>,
        gateways: Vec<Arc<RecordingGateway>>,
        bus_capacity: usize,
        env: MapEnv,
        notifier_on: bool,
    ) -> Self {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let dir = tempfile::tempdir().unwrap();
        let gateway = gateways
            .first()
            .cloned()
            .unwrap_or_else(|| Arc::new(RecordingGateway::new("discord")));
        let mut gateways = gateways;
        if gateways.is_empty() {
            gateways.push(gateway.clone());
        }
        let config_text = format!(
            r#"
[hub]
name = "testhub"
data_dir = '{}'
timezone = "UTC"

[web]
bind = "127.0.0.1:0"
public_url = "http://127.0.0.1:7420"

[agent_server]
bind = "127.0.0.1:0"
public_url = "wss://testhub:7421"

[incidents]
confirm_runs = 1

[llm.primary]
kind = "cli"
cli = {{ preset = "claude" }}

[webhooks.alerts]
kind = "generic"
auth = "hmac"
secret = {{ env = "TEST_SECRET" }}

{extra}
"#,
            dir.path().display()
        );
        let env = env.with("TEST_SECRET", SECRET);
        let (config, _) = domovoi_config::parse(&config_text, &env).unwrap();
        let clock = Arc::new(FakeClock::new(start));
        let mut harness = Self {
            clock,
            gateway,
            notifier: Arc::new(RecordingNotifier::new("webhook")),
            runtime: None,
            config,
            primary,
            gateways,
            bus_capacity,
            notifier_on,
            tasks: Vec::new(),
            _dir: dir,
        };
        harness.assemble().await;
        harness
    }

    async fn assemble(&mut self) {
        let gateways = self
            .gateways
            .iter()
            .map(|gateway| gateway.clone() as Arc<dyn ChatGateway>)
            .collect();
        let runtime = HubRuntime::build_with(
            self.config.clone(),
            HubDeps {
                clock: self.clock.clone(),
                process_executor: None,
            },
            AdapterOverrides {
                primary: Some(self.primary.clone()),
                secondary: None,
                gateways: Some(gateways),
                notifiers: Some(if self.notifier_on {
                    vec![self.notifier.clone()]
                } else {
                    Vec::new()
                }),
                bus_capacity: Some(self.bus_capacity),
            },
        )
        .await
        .unwrap();
        self.runtime = Some(runtime);
    }

    pub fn start_dispatcher(&mut self) {
        let runtime = self.runtime.as_mut().unwrap();
        let events = std::mem::replace(&mut runtime.events, runtime.bus.subscribe());
        let task = tokio::spawn(
            runtime
                .dispatcher
                .clone()
                .run(events, runtime.delivery_shutdown.clone()),
        );
        self.tasks.push(task);
    }

    pub fn start_schedulers(&mut self) {
        let runtime = self.runtime.as_ref().unwrap();
        self.tasks.extend(
            runtime
                .schedulers
                .clone()
                .spawn(runtime.intake_shutdown.clone()),
        );
    }

    pub async fn webhook(&self, body: &str) -> StatusCode {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(SECRET.as_bytes()).unwrap();
        mac.update(body.as_bytes());
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        let mut request = Request::post("/hooks/alerts")
            .body(Body::from(body.to_owned()))
            .unwrap();
        request.headers_mut().insert(
            "x-domovoi-signature",
            HeaderValue::from_str(&signature).unwrap(),
        );
        self.runtime
            .as_ref()
            .unwrap()
            .app
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .status()
    }

    pub async fn incidents(&self) -> Vec<Incident> {
        IncidentRepo::list(
            &*self.runtime.as_ref().unwrap().store,
            &IncidentFilter::default(),
            &Page::first(Page::MAX_LIMIT),
        )
        .await
        .unwrap()
        .items
    }

    pub async fn signals(&self, incident: IncidentId) -> usize {
        self.runtime
            .as_ref()
            .unwrap()
            .store
            .list_for_incident(incident, 100)
            .await
            .unwrap()
            .len()
    }

    pub async fn advisories(&self) -> Vec<Advisory> {
        self.runtime
            .as_ref()
            .unwrap()
            .store
            .list_open()
            .await
            .unwrap()
    }

    pub async fn put_notify_state(&self, incident: IncidentId, state: &NotifyState) {
        NotifyStateRepo::put(
            &*self.runtime.as_ref().unwrap().store,
            incident,
            state,
            self.clock.now(),
        )
        .await
        .unwrap();
    }

    pub async fn restart(mut self) -> Self {
        self.stop_inner().await;
        self.assemble().await;
        self
    }

    pub async fn stop(mut self) {
        self.stop_inner().await;
    }

    async fn stop_inner(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        runtime.intake_shutdown.cancel();
        runtime.agent_server.shutdown();
        runtime.worker_shutdown.cancel();
        runtime.manager_shutdown.cancel();
        runtime.delivery_shutdown.cancel();
        guard(async {
            for task in &mut self.tasks {
                let _ = task.await;
            }
            let _ = runtime.manager_task.await;
            if let Some(task) = runtime.triage_task {
                let _ = task.await;
            }
            let _ = runtime.actions_task.await;
            runtime.store.close().await;
        })
        .await;
        self.tasks.clear();
    }
}

pub struct BlockingLlm {
    pub started: Arc<Notify>,
    release: Arc<Notify>,
    completed: Arc<Mutex<bool>>,
}

impl BlockingLlm {
    pub fn new() -> Self {
        Self {
            started: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
            completed: Arc::new(Mutex::new(false)),
        }
    }

    pub fn completed(&self) -> bool {
        *self.completed.lock().unwrap()
    }

    pub fn releaser(&self) -> Arc<Notify> {
        self.release.clone()
    }
}

#[async_trait]
impl LlmProvider for BlockingLlm {
    fn id(&self) -> &str {
        "blocking"
    }

    fn supports_tools(&self) -> bool {
        false
    }

    async fn complete(
        &self,
        _req: LlmRequest,
    ) -> Result<LlmResponse, domovoi_core::ports::LlmError> {
        self.started.notify_one();
        self.release.notified().await;
        *self.completed.lock().unwrap() = true;
        Ok(LlmResponse {
            parts: vec![Part::Text(REPORT.to_owned())],
            stop: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
}

pub async fn guard<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("runtime test timed out")
}

pub async fn until(mut condition: impl FnMut() -> bool) {
    guard(async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await;
}
