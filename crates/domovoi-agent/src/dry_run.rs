//! `domovoi check run`: every enabled check once, locally, without a hub and without
//! touching the agent's stored check state.

use std::collections::BTreeMap;

use domovoi_checks::commands::Commands;
use domovoi_checks::containers::ContainerDeps;
use domovoi_checks::framework::{CheckContext, CheckState, HostDeps, Registry, run_bounded};
use domovoi_config::AgentConfig;
use domovoi_core::{CheckKey, FactValue, HostId, Severity};
use serde::Serialize;

use crate::checks::check_settings;
use crate::error::AgentError;
use crate::runtime::{AgentDeps, container_policy};

/// One check's result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckRun {
    /// The check's key.
    pub check: String,
    /// Why the check could not run, if it failed.
    pub error: Option<String>,
    /// What it saw, one entry per target.
    pub observations: Vec<ObservationRow>,
    /// Measurements for the digest.
    pub facts: Vec<FactRow>,
}

/// One observation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservationRow {
    /// The target, when the check looks at several.
    pub target: Option<String>,
    /// Whether the condition is present.
    pub firing: bool,
    /// How bad it would be.
    pub severity: Severity,
    /// One line.
    pub summary: String,
}

/// One digest fact.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactRow {
    /// Stable key.
    pub key: String,
    /// Human label.
    pub label: String,
    /// The value as text.
    pub value: String,
    /// Unit, if any.
    pub unit: Option<String>,
}

/// Runs every enabled check once (or only `only`) and returns the results in key order.
/// The host id is a placeholder: nothing leaves the process.
pub async fn run_checks_once(
    config: &AgentConfig,
    deps: &AgentDeps,
    only: Option<&CheckKey>,
) -> Result<Vec<CheckRun>, AgentError> {
    let settings = check_settings(config).map_err(AgentError::Checks)?;
    let containers = deps.container_source.clone().map(|source| ContainerDeps {
        source,
        policy: container_policy(config),
    });
    let registry = Registry::build(
        &settings,
        HostDeps {
            fs_stats: deps.fs_stats.clone(),
            containers,
        },
    )
    .map_err(|e| AgentError::Checks(e.to_string()))?;
    if let Some(key) = only
        && registry.get(key).is_none()
    {
        return Err(AgentError::Checks(format!("no enabled check named {key}")));
    }
    let commands = Commands::resolve(&*deps.hostfs).await;
    let host = HostId::new();
    let mut runs = Vec::new();
    for check in registry.checks() {
        let key = check.key();
        if only.is_some_and(|only| *only != key) {
            continue;
        }
        let check_settings = settings.get(&key).cloned().unwrap_or_default();
        let mut state = CheckState::default();
        let mut ctx = CheckContext {
            host,
            fs: &*deps.hostfs,
            exec: &*deps.executor,
            commands: &commands,
            clock: &*deps.clock,
            settings: &check_settings,
            state: &mut state,
        };
        let run = match run_bounded(check, &mut ctx).await {
            Ok(report) => CheckRun {
                check: key.to_string(),
                error: None,
                observations: report
                    .observations
                    .into_iter()
                    .map(|o| ObservationRow {
                        target: o.target.map(|t| t.as_str().to_owned()),
                        firing: o.firing,
                        severity: o.severity,
                        summary: o.summary,
                    })
                    .collect(),
                facts: report
                    .facts
                    .into_iter()
                    .map(|f| FactRow {
                        key: f.key,
                        label: f.label,
                        value: match f.value {
                            FactValue::Text(text) => text,
                            FactValue::Number(n) => n.to_string(),
                        },
                        unit: f.unit,
                    })
                    .collect(),
            },
            Err(err) => CheckRun {
                check: key.to_string(),
                error: Some(err.to_string()),
                observations: Vec::new(),
                facts: Vec::new(),
            },
        };
        runs.push(run);
    }
    Ok(runs)
}

/// Counts by outcome, for a summary line.
#[must_use]
pub fn summary(runs: &[CheckRun]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for run in runs {
        let label = if run.error.is_some() {
            "error"
        } else if run.observations.iter().any(|o| o.firing) {
            "firing"
        } else {
            "ok"
        };
        *counts.entry(label).or_insert(0) += 1;
    }
    counts
}
