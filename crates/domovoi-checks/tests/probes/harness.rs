//! A check context for probes: real time on a clock that never fires timeouts by itself.

use std::path::PathBuf;

use domovoi_checks::commands::Commands;
use domovoi_checks::framework::{
    Check, CheckContext, CheckReport, CheckSettings, CheckState, Observation,
};
use domovoi_core::ports::Clock;
use domovoi_core::{EvidenceValue, HostId};
use domovoi_testkit::{FakeClock, FakeProcessExecutor, FixtureHostFs};
use jiff::Timestamp;

pub fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

pub struct Harness {
    pub fs: FixtureHostFs,
    pub clock: FakeClock,
    pub settings: CheckSettings,
    pub state: CheckState,
    exec: FakeProcessExecutor,
    commands: Commands,
}

impl Harness {
    /// Certificates are checked against real time, so the fake clock starts at now.
    pub fn new(fs_root: &str) -> Self {
        Self {
            fs: FixtureHostFs::new(fixture(fs_root)),
            clock: FakeClock::new(Timestamp::now()),
            settings: CheckSettings::default(),
            state: CheckState::new(),
            exec: FakeProcessExecutor::new(),
            commands: Commands::default(),
        }
    }

    pub fn clock(&self) -> &dyn Clock {
        &self.clock
    }

    pub async fn run(&mut self, check: &dyn Check) -> CheckReport {
        let mut ctx = CheckContext {
            host: HostId::new(),
            fs: &self.fs,
            exec: &self.exec,
            commands: &self.commands,
            clock: &self.clock,
            settings: &self.settings,
            state: &mut self.state,
        };
        check.run(&mut ctx).await.unwrap()
    }
}

pub fn only(report: &CheckReport) -> &Observation {
    assert_eq!(report.observations.len(), 1, "{report:#?}");
    &report.observations[0]
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
