//! `container.new`: a container outside any stack appeared.
//!
//! - **Fires** once, with info severity, for each new unmanaged container that Docker does
//!   not auto-remove. Throwaway `docker run --rm` containers are never reported.
//! - The first run records the containers already there without reporting them.
//! - **Target:** the container name. **Class:** immediate (it is an event, not a state).
//! - **Evidence:** `image`, `state`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::{SignedDuration, Timestamp};

use super::{ContainerDeps, visible};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, key, target};
use crate::learn::{load_json, save_json};

/// The state key of the containers already seen.
pub const SEEN_STATE: &str = "seen";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Seen {
    bootstrapped: bool,
    names: BTreeMap<String, Timestamp>,
}

/// The `container.new` check.
pub struct NewContainers {
    key: CheckKey,
    deps: ContainerDeps,
}

impl NewContainers {
    /// The check over `deps`.
    pub fn new(deps: ContainerDeps) -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("container.new")?,
            deps,
        })
    }
}

#[async_trait]
impl Check for NewContainers {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Immediate
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let containers = visible(&self.deps).await?;
        let now = ctx.clock.now();
        let mut seen: Seen = load_json(ctx.state, SEEN_STATE);
        let mut report = CheckReport::default();
        for c in containers.iter().filter(|c| !c.managed() && !c.auto_remove) {
            if seen.names.insert(c.name.clone(), now).is_none() && seen.bootstrapped {
                report.observations.push(
                    Observation::firing(
                        target(&c.name),
                        Severity::Info,
                        format!("new container {} from {}", c.name, c.image),
                    )
                    .with_text("image", c.image.clone())
                    .with_text("state", c.state.as_str()),
                );
            }
        }
        let expiry = self.deps.policy.baseline_expiry;
        seen.names
            .retain(|_, last| now.duration_since(*last) <= expiry);
        seen.bootstrapped = true;
        save_json(ctx.state, SEEN_STATE, &seen);
        Ok(report)
    }
}
