//! `unit.failed`: systemd units in the failed state.
//!
//! - **Source:** `systemctl list-units --state=failed`, as JSON where systemd supports
//!   `--output=json` (246 and later), as plain text otherwise.
//! - **Target:** the unit name. **Severity:** warning.
//! - Units that were failed on the previous run and no longer are report ok.
//! - **Evidence:** `load`, `active`, `sub`, `description`.

use std::collections::BTreeSet;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, Observation, key, run_command, target,
};
use crate::learn::{load_json, save_json};

/// The state key of the units failed on the previous run.
pub const FAILED_STATE: &str = "failed";
/// Most failed units reported in one run.
pub const MAX_FAILED: usize = 200;

/// One failed unit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct FailedUnit {
    /// The unit name, such as `backup.service`.
    pub unit: String,
    /// Load state.
    #[serde(default)]
    pub load: String,
    /// Active state.
    #[serde(default)]
    pub active: String,
    /// Sub state.
    #[serde(default)]
    pub sub: String,
    /// The unit's description.
    #[serde(default)]
    pub description: String,
}

/// Parses `systemctl list-units --output=json`; `None` when it is not that JSON.
#[must_use]
pub fn parse_failed_json(output: &str) -> Option<Vec<FailedUnit>> {
    serde_json::from_str(output.trim()).ok()
}

/// Parses `systemctl list-units --plain --no-legend`: `UNIT LOAD ACTIVE SUB DESCRIPTION`.
#[must_use]
pub fn parse_failed_plain(output: &str) -> Vec<FailedUnit> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches('●').trim_start();
            let mut words = line.split_whitespace();
            let unit = words.next()?.to_owned();
            let load = words.next().unwrap_or_default().to_owned();
            let active = words.next().unwrap_or_default().to_owned();
            let sub = words.next().unwrap_or_default().to_owned();
            let description = words.collect::<Vec<_>>().join(" ");
            Some(FailedUnit {
                unit,
                load,
                active,
                sub,
                description,
            })
        })
        .collect()
}

/// The `unit.failed` check.
pub struct FailedUnits {
    key: CheckKey,
}

impl FailedUnits {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("unit.failed")?,
        })
    }
}

async fn failed_units(ctx: &CheckContext<'_>) -> Result<Vec<FailedUnit>, CheckError> {
    let json = run_command(ctx, ctx.commands.systemctl_failed_json()?).await?;
    if json.exit_code == Some(0)
        && !json.truncated
        && let Some(units) = parse_failed_json(&json.stdout)
    {
        return Ok(units);
    }
    let plain = run_command(ctx, ctx.commands.systemctl_failed()?).await?;
    Ok(parse_failed_plain(&plain.stdout))
}

#[async_trait]
impl Check for FailedUnits {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let mut units = failed_units(ctx).await?;
        units.truncate(MAX_FAILED);
        let previous: BTreeSet<String> = load_json(ctx.state, FAILED_STATE);
        let mut report = CheckReport::default();
        let mut now_failed = BTreeSet::new();
        for unit in units {
            now_failed.insert(unit.unit.clone());
            report.observations.push(
                Observation::firing(
                    target(&unit.unit),
                    Severity::Warning,
                    format!("{} failed", unit.unit),
                )
                .with_text("load", unit.load)
                .with_text("active", unit.active)
                .with_text("sub", unit.sub)
                .with_text("description", unit.description),
            );
        }
        for name in previous.difference(&now_failed) {
            report.observations.push(Observation::ok(
                target(name),
                format!("{name} is no longer failed"),
            ));
        }
        save_json(ctx.state, FAILED_STATE, &now_failed);
        Ok(report)
    }
}
