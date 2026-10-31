//! `agent.toml`: configuration for an agent.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use domovoi_core::{CheckKey, Criticality, HostName, Risk};
use schemars::JsonSchema;
use serde::Deserialize;
use url::Url;

use crate::duration::Duration;
use crate::secret::{Secret, Secrets};

/// The whole of `agent.toml`. Only `[agent]` is required.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// Identity of this agent and where it connects.
    pub agent: AgentSection,
    /// What skills this agent is willing to run.
    #[serde(default)]
    pub policy: PolicySection,
    /// The Docker integration.
    #[serde(default)]
    pub docker: DockerSection,
    /// Per-check settings, keyed by check key such as `disk.usage`.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::checks")]
    pub checks: BTreeMap<CheckKey, CheckConfig>,
    /// Container baselines.
    #[serde(default)]
    pub containers: ContainersSection,
    /// A local listener for webhooks from services on this host. Omit to disable.
    #[serde(default)]
    pub local_webhooks: Option<LocalWebhooks>,
}

/// `[agent]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentSection {
    /// This host's name as shown in Domovoi.
    #[schemars(schema_with = "crate::schema::host_name")]
    pub host_name: HostName,
    /// The hub's agent server, such as `wss://hub.example.net:7421`. Must use `wss`.
    pub hub_url: Url,
    /// Where the agent keeps its key and spool. Default `/var/lib/domovoi-agent`.
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    /// Where skill manifests are read from. Default `/etc/domovoi/skills`.
    #[serde(default = "default_skills_dir")]
    pub skills_dir: PathBuf,
    /// Where the host filesystem is mounted; `/host` when running in a container.
    /// Default `/`.
    #[serde(default = "default_host_root")]
    pub host_root: PathBuf,
}

/// `[policy]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicySection {
    /// Risk classes of skills this agent runs. Default `["read", "write"]`; add
    /// `"destructive"` to opt in.
    #[serde(default = "default_allow_risk")]
    #[schemars(schema_with = "crate::schema::risk_list")]
    pub allow_risk: Vec<Risk>,
}

impl Default for PolicySection {
    fn default() -> Self {
        Self {
            allow_risk: default_allow_risk(),
        }
    }
}

/// `[docker]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DockerSection {
    /// Whether container checks and skills are enabled. Default `false`.
    #[serde(default)]
    pub enabled: bool,
    /// `unix:///path/to/docker.sock` or `tcp://host:port` (for a socket proxy). Default
    /// `unix:///var/run/docker.sock`.
    #[serde(default = "default_docker_endpoint")]
    pub endpoint: String,
}

impl Default for DockerSection {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_docker_endpoint(),
        }
    }
}

/// `[checks.<check_key>]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckConfig {
    /// Whether the check runs. Default `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// How often it runs. Default: the check's own interval.
    #[serde(default)]
    pub interval: Option<Duration>,
    /// Check-specific thresholds; each check documents its keys.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::thresholds")]
    pub thresholds: toml::Table,
    /// Overrides how bad a failure is. Default: decided by the check.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::criticality")]
    pub criticality: Option<Criticality>,
}

/// `[containers]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContainersSection {
    /// Container names whose absence is critical.
    #[serde(default)]
    pub critical: Vec<String>,
    /// Container names never checked.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// A container gone this long is dropped from the baseline. Default `"7d"`.
    #[serde(default = "default_baseline_expiry")]
    pub baseline_expiry: Duration,
}

impl Default for ContainersSection {
    fn default() -> Self {
        Self {
            critical: Vec::new(),
            ignore: Vec::new(),
            baseline_expiry: default_baseline_expiry(),
        }
    }
}

/// `[local_webhooks]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalWebhooks {
    /// Listen address. Default `127.0.0.1:7422`.
    #[serde(default = "default_local_bind")]
    pub bind: SocketAddr,
    /// A bearer token callers must send. Optional.
    #[serde(default)]
    pub token: Option<Secret>,
}

impl AgentConfig {
    /// The JSON Schema of `agent.toml`, for the reference documentation.
    #[must_use]
    pub fn json_schema() -> schemars::Schema {
        schemars::schema_for!(AgentConfig)
    }
}

impl Secrets for AgentConfig {
    fn secrets_mut(&mut self, visit: &mut dyn FnMut(&str, &mut Secret)) {
        if let Some(token) = self.local_webhooks.as_mut().and_then(|l| l.token.as_mut()) {
            visit("local_webhooks.token", token);
        }
    }
}

fn default_state_dir() -> PathBuf {
    PathBuf::from("/var/lib/domovoi-agent")
}
fn default_skills_dir() -> PathBuf {
    PathBuf::from("/etc/domovoi/skills")
}
fn default_host_root() -> PathBuf {
    PathBuf::from("/")
}
fn default_allow_risk() -> Vec<Risk> {
    vec![Risk::Read, Risk::Write]
}
fn default_docker_endpoint() -> String {
    "unix:///var/run/docker.sock".to_owned()
}
fn default_baseline_expiry() -> Duration {
    Duration::secs(7 * 24 * 3600)
}
fn default_local_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7422))
}
fn default_true() -> bool {
    true
}
