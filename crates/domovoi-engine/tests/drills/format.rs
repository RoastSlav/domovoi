//! The drill file format: one YAML file per drill in `tests/drills/cases/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use domovoi_core::{Evidence, Verdict};
use serde::Deserialize;
use serde_json::Value;

/// One scored scenario.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drill {
    /// Short identifier, the file name without `.yaml`.
    pub name: String,
    /// What the drill checks, in a sentence or two.
    pub description: String,
    pub setup: Setup,
    /// Canned answers to the model's read tools. The first entry whose `tool` matches and
    /// whose `matches` text appears in the call answers it; anything else gets "no data".
    #[serde(default)]
    pub agent_responses: Vec<AgentResponse>,
    /// Scripted model turns for the deterministic run, in order.
    pub model: Vec<Turn>,
    pub answer_key: AnswerKey,
}

/// The host, its skills and what happened on it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// Starter skills (folder names under `examples/skills`) installed on the host.
    #[serde(default)]
    pub skills: Vec<String>,
    /// The incident triage is asked about.
    pub incident: IncidentSpec,
    /// Later signals on that incident, each some seconds after the previous one.
    #[serde(default)]
    pub signals: Vec<SignalSpec>,
    /// Other open incidents on the host that are not the cause.
    #[serde(default)]
    pub decoys: Vec<IncidentSpec>,
    /// A value planted in the evidence or tool output that must never reach the model.
    pub secret: String,
}

/// An open incident and its first signal.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncidentSpec {
    pub check: String,
    #[serde(default)]
    pub target: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub evidence: Evidence,
}

/// A later signal on the drill's incident.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalSpec {
    pub after_secs: u64,
    pub summary: String,
    #[serde(default)]
    pub evidence: Evidence,
}

/// One canned tool answer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentResponse {
    /// A read tool's name, such as `docker_logs`.
    pub tool: String,
    /// Text that must appear in the call (its arguments) for this answer to apply.
    #[serde(default)]
    pub matches: Option<String>,
    pub output: String,
}

/// One scripted model turn: `calls: [...]` or `report: {...}`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Turn {
    /// Call read tools.
    Calls { calls: Vec<Call> },
    /// Answer with this report.
    Report { report: Value },
}

/// One tool call in a scripted turn.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub tool: String,
    #[serde(default = "empty_object")]
    pub input: Value,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// What a good triage concludes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerKey {
    pub verdict: Verdict,
    /// The one correct proposal, or none.
    #[serde(default)]
    pub proposal: Option<ExpectedProposal>,
    /// Whether making no proposal is as good as making `proposal`.
    #[serde(default)]
    pub proposal_optional: bool,
    /// Targets of decoy incidents that must not be named as the cause.
    #[serde(default)]
    pub decoys: Vec<String>,
    /// Words the report's summary, cause or details must contain, such as "injection".
    #[serde(default)]
    pub mentions: Vec<String>,
}

/// A proposal the answer key expects.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedProposal {
    pub skill: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

/// The folder holding the drill files.
pub fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/drills/cases")
}

/// Every drill, sorted by file name.
pub fn load_all() -> Vec<Drill> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(cases_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            let drill: Drill = serde_norway::from_str(&text)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let stem = path.file_stem().unwrap().to_str().unwrap();
            assert_eq!(
                drill.name,
                stem,
                "{}: name must match the file",
                path.display()
            );
            drill
        })
        .collect()
}
