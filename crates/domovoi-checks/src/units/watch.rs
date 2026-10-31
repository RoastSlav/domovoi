//! `unit.watch`: units the operator listed must stay active.
//!
//! - **Settings:** `units` (the units to watch), `critical` (units whose failure is
//!   critical; `criticality = "critical"` makes all of them critical) and `restarts`
//!   (default 3).
//! - **Source:** `systemctl show` with `ActiveState`, `SubState`, `NRestarts` and `Result`.
//! - **Targets:** the unit name (not active → warning or critical) and `<unit>:restarts`
//!   (`NRestarts` grew by at least `restarts` within an hour → warning).
//! - **Evidence:** `active`, `sub`, `result`; `restarts` and `window` for restart loops.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Criticality, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use crate::commands::{UnitName, UnitProperty};
use crate::containers::RESTART_WINDOW;
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, Observation, int, key, run_command, target,
};
use crate::learn::{Windows, load_json, save_json};

/// The state key of the per-unit restart windows.
pub const WINDOWS_STATE: &str = "windows";
/// Most units one check watches.
pub const MAX_WATCHED: usize = 64;

/// `[checks."unit.watch"]` thresholds.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WatchThresholds {
    /// The units to watch.
    pub units: Vec<String>,
    /// Units whose failure is critical.
    pub critical: Vec<String>,
    /// `NRestarts` growth within an hour that counts as a restart loop.
    pub restarts: u64,
}

impl Default for WatchThresholds {
    fn default() -> Self {
        Self {
            units: Vec::new(),
            critical: Vec::new(),
            restarts: 3,
        }
    }
}

/// The properties `unit.watch` reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitProps {
    /// `ActiveState`, such as `active` or `failed`.
    pub active: String,
    /// `SubState`, such as `running`.
    pub sub: String,
    /// `NRestarts`, when systemd reports it.
    pub restarts: Option<u64>,
    /// `Result`, such as `success` or `exit-code`.
    pub result: String,
}

/// Parses `systemctl show` `Key=Value` lines.
#[must_use]
pub fn parse_show(output: &str) -> UnitProps {
    let values: BTreeMap<&str, &str> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let text = |k: &str| values.get(k).copied().unwrap_or_default().to_owned();
    UnitProps {
        active: text("ActiveState"),
        sub: text("SubState"),
        restarts: values.get("NRestarts").and_then(|v| v.parse().ok()),
        result: text("Result"),
    }
}

/// The `unit.watch` check.
pub struct WatchedUnits {
    key: CheckKey,
}

impl WatchedUnits {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("unit.watch")?,
        })
    }
}

fn properties() -> Result<Vec<UnitProperty>, CheckError> {
    ["ActiveState", "SubState", "NRestarts", "Result"]
        .into_iter()
        .map(|p| UnitProperty::parse(p).map_err(CheckError::from))
        .collect()
}

#[async_trait]
impl Check for WatchedUnits {
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
        let thresholds: WatchThresholds = ctx.settings.thresholds()?;
        if thresholds.units.len() > MAX_WATCHED {
            return Err(CheckError::Settings(format!(
                "at most {MAX_WATCHED} units can be watched"
            )));
        }
        let units = thresholds
            .units
            .iter()
            .map(|u| UnitName::parse(u))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CheckError::Settings(e.to_string()))?;
        let props = properties()?;
        let all_critical = ctx.settings.criticality == Some(Criticality::Critical);
        let now = ctx.clock.now();
        let mut windows: Windows = load_json(ctx.state, WINDOWS_STATE);
        let mut report = CheckReport::default();
        for unit in &units {
            let name = unit.as_str();
            let output = run_command(ctx, ctx.commands.systemctl_show(unit, &props)?).await?;
            let state = parse_show(&output.stdout);
            let observation = if state.active == "active" {
                Observation::ok(target(name), format!("{name} is active"))
            } else {
                let severity = if all_critical || thresholds.critical.iter().any(|c| c == name) {
                    Severity::Critical
                } else {
                    Severity::Warning
                };
                let shown = if state.active.is_empty() {
                    "unknown"
                } else {
                    &state.active
                };
                Observation::firing(target(name), severity, format!("{name} is {shown}"))
            };
            report.observations.push(
                observation
                    .with_text("active", state.active.clone())
                    .with_text("sub", state.sub.clone())
                    .with_text("result", state.result.clone()),
            );
            if let Some(count) = state.restarts {
                let window = windows.entry(name.to_owned()).or_default();
                window.record(now, count, RESTART_WINDOW);
                let restarts = window.increase();
                let restart_target = target(&format!("{name}:restarts"));
                let observation = if restarts >= thresholds.restarts {
                    Observation::firing(
                        restart_target,
                        Severity::Warning,
                        format!("{name} restarted {restarts} times in the last hour"),
                    )
                } else {
                    Observation::ok(restart_target, format!("{name} is not restarting"))
                };
                report.observations.push(
                    observation
                        .with("restarts", int(restarts))
                        .with_text("window", "1h"),
                );
            }
        }
        windows.retain(|name, _| units.iter().any(|u| u.as_str() == name));
        save_json(ctx.state, WINDOWS_STATE, &windows);
        Ok(report)
    }
}
