//! `[policy]` in `hub.toml`. The same table name also holds `allow_risk` in `agent.toml`;
//! the two files read different keys from it (see `env::AGENT_SECTIONS`).

use schemars::JsonSchema;
use serde::Deserialize;

/// `[policy]`. Record, for every Execution step, what the active policy and its shadow
/// rules would decide. Nothing is enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HubPolicySection {
    /// Record, for every Execution step, what the active policy and its shadow rules would
    /// decide. Nothing is enforced. Default `false`.
    #[serde(default)]
    pub shadow: bool,
}
