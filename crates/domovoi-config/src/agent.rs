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

/// `[docker]`: the container runtime integration. It speaks the Docker Engine API, which
/// Docker, Podman and compatible engines answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DockerSection {
    /// `"auto"`, `true` or `false`. `"auto"` (the default) uses an engine that answers on
    /// the usual sockets and lets the hub switch it on or off for this host; `true` and
    /// `false` fix the choice here and the hub cannot change it.
    #[serde(default)]
    pub enabled: DockerMode,
    /// `unix:///path/to/engine.sock` or `tcp://host:port` (for a socket proxy). Default:
    /// the first of Docker's socket, rootless Docker's, Podman's and rootless Podman's that
    /// answers.
    #[serde(default)]
    pub endpoint: Option<String>,
}

impl Default for DockerSection {
    fn default() -> Self {
        Self {
            enabled: DockerMode::Auto,
            endpoint: None,
        }
    }
}

/// How `[docker] enabled` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockerMode {
    /// Use an engine that answers; the hub may switch it on or off.
    #[default]
    Auto,
    /// Always use the engine at the endpoint; the hub cannot switch it off.
    On,
    /// Never use an engine; the hub cannot switch it on.
    Off,
}

impl<'de> Deserialize<'de> for DockerMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = DockerMode;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("true, false or \"auto\"")
            }

            fn visit_bool<E>(self, value: bool) -> Result<DockerMode, E> {
                Ok(if value {
                    DockerMode::On
                } else {
                    DockerMode::Off
                })
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<DockerMode, E> {
                match value {
                    "auto" => Ok(DockerMode::Auto),
                    "true" => Ok(DockerMode::On),
                    "false" => Ok(DockerMode::Off),
                    other => Err(E::invalid_value(serde::de::Unexpected::Str(other), &self)),
                }
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

impl JsonSchema for DockerMode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "DockerMode".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "anyOf": [{ "type": "boolean" }, { "type": "string", "enum": ["auto"] }]
        })
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
    /// Allow a `bind` address other than loopback. Default false: services on this host
    /// reach the relay, and nothing else does.
    #[serde(default)]
    pub allow_non_loopback: bool,
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

/// The default directories of `[agent]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDirs {
    /// `state_dir`.
    pub state_dir: PathBuf,
    /// `skills_dir`.
    pub skills_dir: PathBuf,
    /// `host_root`.
    pub host_root: PathBuf,
}

impl AgentSection {
    /// The directories `[agent]` uses when they are not set.
    #[must_use]
    pub fn default_dirs() -> AgentDirs {
        AgentDirs {
            state_dir: default_state_dir(),
            skills_dir: default_skills_dir(),
            host_root: default_host_root(),
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
fn default_baseline_expiry() -> Duration {
    Duration::secs(7 * 24 * 3600)
}
fn default_local_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7422))
}
fn default_true() -> bool {
    true
}

impl crate::env::OverrideScope for AgentConfig {
    fn foreign_sections() -> &'static [&'static str] {
        &crate::env::HUB_SECTIONS
    }
}
