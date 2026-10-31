//! `container.restart_loop`: a container keeps restarting.
//!
//! - **Source:** Docker's restart count, sampled every run into a one-hour window kept in
//!   state. A recreated container (the count drops) counts from zero.
//! - **Fires** when the count grew by at least `restarts` (default 3) within the window.
//! - **Target:** the container name. **Severity:** warning.
//! - **Evidence:** `restarts` (growth in the window), `window`, `restart_count`.

use std::collections::BTreeSet;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{ContainerDeps, visible};
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, Observation, int, key, target,
};
use crate::learn::{Windows, load_json, save_json};

/// The state key of the per-container restart windows.
pub const WINDOWS_STATE: &str = "windows";
/// The window restarts are counted in.
pub const RESTART_WINDOW: SignedDuration = SignedDuration::from_hours(1);

/// `[checks."container.restart_loop"]` thresholds.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RestartThresholds {
    /// Restarts within the window that count as a loop.
    pub restarts: u64,
}

impl Default for RestartThresholds {
    fn default() -> Self {
        Self { restarts: 3 }
    }
}

/// The `container.restart_loop` check.
pub struct RestartLoop {
    key: CheckKey,
    deps: ContainerDeps,
}

impl RestartLoop {
    /// The check over `deps`.
    pub fn new(deps: ContainerDeps) -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("container.restart_loop")?,
            deps,
        })
    }
}

#[async_trait]
impl Check for RestartLoop {
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
        let thresholds: RestartThresholds = ctx.settings.thresholds()?;
        let containers = visible(&self.deps).await?;
        let now = ctx.clock.now();
        let mut windows: Windows = load_json(ctx.state, WINDOWS_STATE);
        let mut report = CheckReport::default();
        let mut present = BTreeSet::new();
        for c in containers.iter().filter(|c| !c.auto_remove) {
            present.insert(c.name.clone());
            let window = windows.entry(c.name.clone()).or_default();
            window.record(now, c.restart_count, RESTART_WINDOW);
            let restarts = window.increase();
            let observation = if restarts >= thresholds.restarts {
                Observation::firing(
                    target(&c.name),
                    Severity::Warning,
                    format!("{} restarted {restarts} times in the last hour", c.name),
                )
            } else {
                Observation::ok(target(&c.name), format!("{} is not restarting", c.name))
            };
            report.observations.push(
                observation
                    .with("restarts", int(restarts))
                    .with_text("window", "1h")
                    .with("restart_count", int(c.restart_count)),
            );
        }
        windows.retain(|name, _| present.contains(name));
        save_json(ctx.state, WINDOWS_STATE, &windows);
        Ok(report)
    }
}
