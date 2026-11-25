//! `[policy]` in `hub.toml`. The same table name also holds `allow_risk` in `agent.toml`;
//! the two files read different keys from it (see `env::AGENT_SECTIONS`).

use schemars::JsonSchema;
use serde::Deserialize;

use crate::duration::Duration;

/// `[policy]`. Shadow recording and the guardrails of bounded unattended execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HubPolicySection {
    /// Record, for every Execution step, what the active policy and its shadow rules would
    /// decide. Nothing is enforced by this switch. Default `false`.
    #[serde(default)]
    pub shadow: bool,
    /// How long a policy permit stays valid, capped by the step's own approval window.
    /// 30 seconds to 1 hour. Default `5m`.
    #[serde(default = "default_permit_ttl")]
    pub permit_ttl: Duration,
    /// How many `Failed` or `OutcomeUnknown` results of permit-authorized steps suspend a rule
    /// within `circuit_window`. 1 to 20. Default 2.
    #[serde(default = "default_circuit_failures")]
    pub circuit_failures: u32,
    /// The window the circuit breaker counts failures in. 1 hour to 30 days. Default `24h`.
    #[serde(default = "default_circuit_window")]
    pub circuit_window: Duration,
    /// How many permits the hub issues per rolling hour, all rules together. 0 to 120; `0`
    /// means no permit is ever issued. Default 12.
    #[serde(default = "default_max_permits_per_hour")]
    pub max_permits_per_hour: u32,
}

impl Default for HubPolicySection {
    fn default() -> Self {
        Self {
            shadow: false,
            permit_ttl: default_permit_ttl(),
            circuit_failures: default_circuit_failures(),
            circuit_window: default_circuit_window(),
            max_permits_per_hour: default_max_permits_per_hour(),
        }
    }
}

fn default_permit_ttl() -> Duration {
    Duration::secs(5 * 60)
}

const fn default_circuit_failures() -> u32 {
    2
}

fn default_circuit_window() -> Duration {
    Duration::secs(24 * 3600)
}

const fn default_max_permits_per_hour() -> u32 {
    12
}
