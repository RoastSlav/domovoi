//! `container.unhealthy`: a container's health check is failing.
//!
//! - **Target:** the container name. **Severity:** warning.
//! - Containers with a passing or starting health check report ok; containers without a
//!   health check report nothing.
//! - **Evidence:** `image`, `state`.

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{ContainerDeps, Health, visible};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, key, target};

/// The `container.unhealthy` check.
pub struct UnhealthyContainers {
    key: CheckKey,
    deps: ContainerDeps,
}

impl UnhealthyContainers {
    /// The check over `deps`.
    pub fn new(deps: ContainerDeps) -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("container.unhealthy")?,
            deps,
        })
    }
}

#[async_trait]
impl Check for UnhealthyContainers {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, _ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let mut report = CheckReport::default();
        for c in visible(&self.deps).await? {
            let Some(health) = c.health else { continue };
            let observation = if health == Health::Unhealthy {
                Observation::firing(
                    target(&c.name),
                    Severity::Warning,
                    format!("{} is unhealthy", c.name),
                )
            } else {
                Observation::ok(target(&c.name), format!("{} is healthy", c.name))
            };
            report.observations.push(
                observation
                    .with_text("image", c.image)
                    .with_text("state", c.state.as_str()),
            );
        }
        Ok(report)
    }
}
