//! A check context over a fixture tree, a fake executor and a fake clock.

use std::path::PathBuf;
use std::sync::Arc;

use domovoi_checks::commands::{Commands, ToolHost};
use domovoi_checks::framework::{
    Check, CheckContext, CheckReport, CheckSettings, CheckState, run_bounded,
};
use domovoi_core::ports::{ProcessOutput, ProcessSpec};
use domovoi_core::{EvidenceValue, HostId, Severity, SignalClass};
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, FakeProcessExecutor, FixtureHostFs};
use uuid::Uuid;

pub fn fixture(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/system")
        .join(scenario)
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
    pub async fn new(scenario: &str) -> Self {
        let fs = FixtureHostFs::new(fixture(scenario));
        let commands = Commands::resolve(&fs, ToolHost::Host).await;
        Self {
            host: HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00b1)),
            fs,
            exec: FakeProcessExecutor::new(),
            commands,
            clock: Arc::new(FakeClock::new(T0)),
            settings: CheckSettings::default(),
            state: CheckState::new(),
        }
    }

    pub fn with_thresholds(mut self, thresholds: serde_json::Value) -> Self {
        self.settings = CheckSettings::with_thresholds(thresholds);
        self
    }

    pub async fn try_run(
        &mut self,
        check: &dyn Check,
    ) -> Result<CheckReport, domovoi_checks::framework::CheckError> {
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

/// The observation for `target` (`None` for a host-wide one).
pub fn find<'a>(
    report: &'a CheckReport,
    target: Option<&str>,
) -> &'a domovoi_checks::framework::Observation {
    report
        .observations
        .iter()
        .find(|o| o.target.as_ref().map(|t| t.as_str()) == target)
        .unwrap_or_else(|| panic!("no observation for {target:?} in {report:#?}"))
}

pub fn assert_firing(report: &CheckReport, target: Option<&str>, severity: Severity) {
    let obs = find(report, target);
    assert!(obs.firing, "{target:?} should fire: {obs:#?}");
    assert_eq!(obs.severity, severity, "{target:?}");
}

pub fn assert_ok(report: &CheckReport, target: Option<&str>) {
    let obs = find(report, target);
    assert!(!obs.firing, "{target:?} should not fire: {obs:#?}");
}

pub fn number(obs: &domovoi_checks::framework::Observation, key: &str) -> f64 {
    match obs.evidence.get(key) {
        Some(EvidenceValue::Float(v)) => *v,
        Some(EvidenceValue::Int(v)) => *v as f64,
        other => panic!("evidence {key} is {other:?}"),
    }
}

pub fn text<'a>(obs: &'a domovoi_checks::framework::Observation, key: &str) -> &'a str {
    match obs.evidence.get(key) {
        Some(EvidenceValue::Text(v)) => v,
        other => panic!("evidence {key} is {other:?}"),
    }
}

pub fn is_immediate(obs: &domovoi_checks::framework::Observation, check: &dyn Check) -> bool {
    obs.class.unwrap_or(check.class()) == SignalClass::Immediate
}

pub fn output(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        ..ProcessOutput::default()
    }
}

pub fn args(spec: &ProcessSpec) -> Vec<&str> {
    spec.args.iter().map(String::as_str).collect()
}
