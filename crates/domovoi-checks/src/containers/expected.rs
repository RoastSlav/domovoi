//! `container.expected_down`: a learned container is missing or not running.
//!
//! - **Baseline:** a managed, non-auto-remove container seen running in two consecutive
//!   runs becomes expected. One gone for [`ContainerPolicy::baseline_expiry`] is dropped
//!   and reported as a `baseline.expired` fact for an advisory.
//! - **Target:** the container name. **Severity:** critical for names in
//!   [`ContainerPolicy::critical`], warning otherwise.
//! - **Evidence:** `state` (`missing` when gone), `image`, `last_seen`.
//! - **Facts:** `containers.expected_running` and `container.<name>.image_age_days`.
//! - **Events:** each run also appends container events to a 24-hour log in this check's
//!   state ([`EVENTS_STATE`]) for the Docker events read tool and triage context.
//!
//! [`ContainerPolicy::baseline_expiry`]: super::ContainerPolicy::baseline_expiry
//! [`ContainerPolicy::critical`]: super::ContainerPolicy::critical

use std::collections::VecDeque;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::{SignedDuration, Timestamp};

use super::{ContainerDeps, ContainerEvent, RunState, visible};
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, CheckState, Observation, key, number_fact,
    target, text_fact,
};
use crate::learn::{Baseline, load_json, save_json};

/// The state key of the baseline.
pub const BASELINE_STATE: &str = "baseline";
/// The state key of the 24-hour container event log.
pub const EVENTS_STATE: &str = "events";
/// How far back the event log reaches.
pub const EVENT_WINDOW: SignedDuration = SignedDuration::from_hours(24);
/// Most events the log keeps.
pub const MAX_LOGGED_EVENTS: usize = 500;

/// Container events from the last 24 hours, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct EventLog {
    /// The end of the last span read from the runtime.
    pub read_until: Option<Timestamp>,
    /// The events.
    pub events: VecDeque<ContainerEvent>,
}

impl EventLog {
    /// The log kept in `state` of the `container.expected_down` check.
    #[must_use]
    pub fn from_state(state: &CheckState) -> Self {
        load_json(state, EVENTS_STATE)
    }

    /// Appends `events` read up to `until` and drops old ones.
    pub fn append(&mut self, events: Vec<ContainerEvent>, until: Timestamp) {
        self.events.extend(events);
        while self
            .events
            .front()
            .is_some_and(|e| until.duration_since(e.at) > EVENT_WINDOW)
        {
            self.events.pop_front();
        }
        while self.events.len() > MAX_LOGGED_EVENTS {
            self.events.pop_front();
        }
        self.read_until = Some(until);
    }
}

/// The `container.expected_down` check.
pub struct ExpectedContainers {
    key: CheckKey,
    deps: ContainerDeps,
}

impl ExpectedContainers {
    /// The check over `deps`.
    pub fn new(deps: ContainerDeps) -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("container.expected_down")?,
            deps,
        })
    }

    async fn record_events(&self, state: &mut CheckState, now: Timestamp) {
        let mut log = EventLog::from_state(state);
        let since = log
            .read_until
            .filter(|t| now.duration_since(*t) < EVENT_WINDOW)
            .unwrap_or_else(|| {
                now.checked_sub(SignedDuration::from_hours(1))
                    .unwrap_or(now)
            });
        match self.deps.source.events_since(since, now).await {
            Ok(events) => {
                log.append(events, now);
                save_json(state, EVENTS_STATE, &log);
            }
            Err(err) => tracing::warn!(%err, "could not read container events"),
        }
    }
}

#[async_trait]
impl Check for ExpectedContainers {
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
        let containers = visible(&self.deps).await?;
        let now = ctx.clock.now();
        let mut baseline = Baseline::load(ctx.state, BASELINE_STATE);
        let running = containers
            .iter()
            .filter(|c| c.learnable() && c.state == RunState::Running)
            .map(|c| c.name.as_str());
        baseline.observe(running, now);
        let expired = baseline.expire(self.deps.policy.baseline_expiry, now);

        let mut report = CheckReport::default();
        let (mut expected, mut up) = (0_u32, 0_u32);
        for (name, entry) in baseline.expected() {
            expected += 1;
            let current = containers.iter().find(|c| c.name == name);
            let observation = match current {
                Some(c) if c.state == RunState::Running => {
                    up += 1;
                    Observation::ok(target(name), format!("{name} is running"))
                }
                _ => {
                    let severity = if self.deps.policy.critical.contains(name) {
                        Severity::Critical
                    } else {
                        Severity::Warning
                    };
                    let state = current.map_or("missing", |c| c.state.as_str());
                    let mut obs = Observation::firing(
                        target(name),
                        severity,
                        format!("expected container {name} is {state}"),
                    )
                    .with_text("state", state)
                    .with_text("last_seen", entry.last_seen.to_string());
                    if let Some(c) = current {
                        obs = obs.with_text("image", c.image.clone());
                    }
                    obs
                }
            };
            report.observations.push(observation);
        }
        baseline.save(ctx.state, BASELINE_STATE);

        if expected > 0 {
            report.facts.push(text_fact(
                ctx.host,
                "containers.expected_running",
                "Expected containers running",
                format!("{up}/{expected} expected containers running"),
                now,
            ));
        }
        for c in containers.iter().filter(|c| !c.auto_remove) {
            if let Some(built) = c.image_created {
                let days = now.duration_since(built).as_secs_f64() / 86_400.0;
                report.facts.push(number_fact(
                    ctx.host,
                    format!("container.{}.image_age_days", c.name),
                    format!("Image age of {}", c.name),
                    days.max(0.0),
                    "days",
                    now,
                ));
            }
        }
        for name in expired {
            report.facts.push(text_fact(
                ctx.host,
                "baseline.expired",
                "Container dropped from the baseline",
                name,
                now,
            ));
        }
        self.record_events(ctx.state, now).await;
        Ok(report)
    }
}
