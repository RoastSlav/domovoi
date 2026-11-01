//! A check context over fixture files, a fake executor, a fake clock and a fake Docker.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_checks::commands::{Commands, ToolHost};
use domovoi_checks::containers::{
    ContainerDeps, ContainerError, ContainerEvent, ContainerInfo, ContainerPolicy, ContainerSource,
    MANAGED_LABEL, RunState,
};
use domovoi_checks::framework::{
    Check, CheckContext, CheckError, CheckReport, CheckSettings, CheckState, Observation,
    run_bounded,
};
use domovoi_core::ports::ProcessOutput;
use domovoi_core::{EvidenceValue, HostId, Severity, SignalClass};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, FakeProcessExecutor, FixtureHostFs};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

pub fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

pub fn read_fixture(path: &str) -> String {
    std::fs::read_to_string(fixture(path)).unwrap()
}

pub struct Harness {
    pub host: HostId,
    pub fs: FixtureHostFs,
    pub exec: FakeProcessExecutor,
    pub commands: Commands,
    pub clock: Arc<FakeClock>,
    pub settings: CheckSettings,
    pub state: CheckState,
}

impl Harness {
    /// A harness whose host files come from `tests/fixtures/<fs_root>` and whose tools are
    /// all installed.
    pub async fn new(fs_root: &str) -> Self {
        Self::with_tools(fs_root, ToolHost::Host).await
    }

    /// Like [`Harness::new`], but as an agent running in a container: every tool is in the
    /// agent's filesystem, yet journalctl and systemctl cannot reach the host.
    pub async fn in_container(fs_root: &str) -> Self {
        Self::with_tools(fs_root, ToolHost::Container).await
    }

    async fn with_tools(fs_root: &str, mode: ToolHost) -> Self {
        let tools = FixtureHostFs::new(fixture("system/tools"));
        Self {
            host: HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00c1)),
            fs: FixtureHostFs::new(fixture(fs_root)),
            exec: FakeProcessExecutor::new(),
            commands: Commands::resolve(&tools, mode).await,
            clock: Arc::new(FakeClock::new(T0)),
            settings: CheckSettings::default(),
            state: CheckState::new(),
        }
    }

    pub fn with_thresholds(mut self, thresholds: serde_json::Value) -> Self {
        self.settings = CheckSettings::with_thresholds(thresholds);
        self
    }

    pub fn use_fs(&mut self, fs_root: &str) {
        self.fs = FixtureHostFs::new(fixture(fs_root));
    }

    pub fn advance(&self, by: SignedDuration) {
        self.clock.advance(by);
    }

    pub async fn try_run(&mut self, check: &dyn Check) -> Result<CheckReport, CheckError> {
        let mut ctx = CheckContext {
            host: self.host,
            fs: &self.fs,
            exec: &self.exec,
            commands: &self.commands,
            clock: &*self.clock,
            settings: &self.settings,
            state: &mut self.state,
        };
        run_bounded(check, &mut ctx).await
    }

    pub async fn run(&mut self, check: &dyn Check) -> CheckReport {
        self.try_run(check).await.unwrap()
    }
}

pub fn output(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        ..ProcessOutput::default()
    }
}

pub fn targets(report: &CheckReport) -> Vec<&str> {
    report
        .observations
        .iter()
        .map(|o| o.target.as_ref().map_or("", |t| t.as_str()))
        .collect()
}

pub fn firing_targets(report: &CheckReport) -> Vec<&str> {
    report
        .observations
        .iter()
        .filter(|o| o.firing)
        .map(|o| o.target.as_ref().map_or("", |t| t.as_str()))
        .collect()
}

pub fn find<'a>(report: &'a CheckReport, target: &str) -> &'a Observation {
    report
        .observations
        .iter()
        .find(|o| o.target.as_ref().map(|t| t.as_str()) == Some(target))
        .unwrap_or_else(|| panic!("no observation for {target} in {report:#?}"))
}

pub fn assert_firing(report: &CheckReport, target: &str, severity: Severity) {
    let obs = find(report, target);
    assert!(obs.firing, "{target} should fire: {obs:#?}");
    assert_eq!(obs.severity, severity, "{target}");
}

pub fn assert_ok(report: &CheckReport, target: &str) {
    let obs = find(report, target);
    assert!(!obs.firing, "{target} should not fire: {obs:#?}");
}

pub fn number(obs: &Observation, key: &str) -> f64 {
    match obs.evidence.get(key) {
        Some(EvidenceValue::Float(v)) => *v,
        Some(EvidenceValue::Int(v)) => *v as f64,
        other => panic!("evidence {key} is {other:?}"),
    }
}

pub fn text<'a>(obs: &'a Observation, key: &str) -> &'a str {
    match obs.evidence.get(key) {
        Some(EvidenceValue::Text(v)) => v,
        other => panic!("evidence {key} is {other:?}"),
    }
}

pub fn is_immediate(obs: &Observation, check: &dyn Check) -> bool {
    obs.class.unwrap_or(check.class()) == SignalClass::Immediate
}

/// A container source whose containers and events the test sets.
#[derive(Default)]
pub struct FakeDocker {
    containers: Mutex<Vec<ContainerInfo>>,
    events: Mutex<Vec<ContainerEvent>>,
}

impl FakeDocker {
    pub fn set(&self, containers: Vec<ContainerInfo>) {
        *self.containers.lock().unwrap() = containers;
    }

    pub fn set_events(&self, events: Vec<ContainerEvent>) {
        *self.events.lock().unwrap() = events;
    }
}

#[async_trait]
impl ContainerSource for FakeDocker {
    async fn list(&self) -> Result<Vec<ContainerInfo>, ContainerError> {
        Ok(self.containers.lock().unwrap().clone())
    }

    async fn events_since(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<ContainerEvent>, ContainerError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.at >= since && e.at <= until)
            .cloned()
            .collect())
    }

    async fn inspect(&self, name: &str) -> Result<serde_json::Value, ContainerError> {
        Ok(serde_json::json!({ "Name": format!("/{name}") }))
    }

    async fn logs(&self, name: &str, lines: u16) -> Result<String, ContainerError> {
        Ok(format!(
            "{lines} lines of {name}
"
        ))
    }
}

pub fn deps(docker: &Arc<FakeDocker>, policy: ContainerPolicy) -> ContainerDeps {
    ContainerDeps {
        source: docker.clone(),
        policy,
    }
}

/// A running container; managed ones carry the compose project label.
pub fn container(name: &str, managed: bool) -> ContainerInfo {
    let mut labels = BTreeMap::new();
    if managed {
        labels.insert(MANAGED_LABEL.to_owned(), "stack".to_owned());
    }
    ContainerInfo {
        name: name.to_owned(),
        image: format!("example/{name}:1.0"),
        image_created: Some(T0 - SignedDuration::from_hours(24 * 30)),
        state: RunState::Running,
        health: None,
        restart_count: 0,
        labels,
        auto_remove: false,
        started_at: Some(T0 - SignedDuration::from_hours(1)),
    }
}

pub fn stopped(mut c: ContainerInfo) -> ContainerInfo {
    c.state = RunState::Exited;
    c
}
