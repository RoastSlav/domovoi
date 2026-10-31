//! `hub.toml`: configuration for the hub.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use domovoi_core::HostName;
use schemars::JsonSchema;
use serde::Deserialize;
use url::Url;

use crate::duration::Duration;
use crate::secret::{Secret, Secrets};
use crate::time::{LocalTime, TimeZone};

mod llm;

pub use llm::{
    CliConfig, CliPreset, LlmKind, LlmProvider, LlmSection, Price, TriageLimits, TriageSection,
};

/// The whole of `hub.toml`. Every section is optional; omitted keys take the defaults
/// documented on each field.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HubConfig {
    /// Identity and storage of this hub.
    #[serde(default)]
    pub hub: HubSection,
    /// The web UI and HTTP API listener.
    #[serde(default)]
    pub web: WebSection,
    /// The listener agents connect to.
    #[serde(default)]
    pub agent_server: AgentServerSection,
    /// Language model providers used by triage.
    #[serde(default)]
    pub llm: LlmSection,
    /// Automatic triage of new incidents.
    #[serde(default)]
    pub triage: TriageSection,
    /// How signals become incidents.
    #[serde(default)]
    pub incidents: IncidentsSection,
    /// When people are notified.
    #[serde(default)]
    pub notify: NotifySection,
    /// The Discord bot. Omit to run without Discord.
    #[serde(default)]
    pub discord: Option<DiscordSection>,
    /// Inbound webhooks, keyed by the name used in their URL.
    #[serde(default)]
    pub webhooks: BTreeMap<String, WebhookConfig>,
    /// Network probes run from the hub.
    #[serde(default)]
    pub probes: Vec<Probe>,
    /// A dead man's switch pinged while the hub is healthy. Omit to disable.
    #[serde(default)]
    pub heartbeat: Option<HeartbeatSection>,
    /// Local database backups.
    #[serde(default)]
    pub backup: BackupSection,
    /// Secret hygiene.
    #[serde(default)]
    pub secrets: SecretsSection,
}

/// `[hub]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HubSection {
    /// Display name of this hub. Default `"domovoi"`.
    #[serde(default = "default_hub_name")]
    pub name: String,
    /// Where the database and state live. Default `/var/lib/domovoi`.
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Time zone for quiet hours, digests and day boundaries. Default `"UTC"`.
    #[serde(default = "TimeZone::utc")]
    pub timezone: TimeZone,
}

impl Default for HubSection {
    fn default() -> Self {
        Self {
            name: default_hub_name(),
            data_dir: default_data_dir(),
            timezone: TimeZone::utc(),
        }
    }
}

/// `[web]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebSection {
    /// Listen address. Default `127.0.0.1:7420`.
    #[serde(default = "default_web_bind")]
    pub bind: SocketAddr,
    /// The URL people use to reach the UI, for links in chat. Optional.
    #[serde(default)]
    pub public_url: Option<Url>,
    /// Reverse proxies whose `X-Forwarded-For` is trusted. Default none.
    #[serde(default)]
    pub trusted_proxies: Vec<IpAddr>,
}

impl Default for WebSection {
    fn default() -> Self {
        Self {
            bind: default_web_bind(),
            public_url: None,
            trusted_proxies: Vec::new(),
        }
    }
}

/// `[agent_server]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentServerSection {
    /// Listen address for agent connections. Default `0.0.0.0:7421`.
    #[serde(default = "default_agent_bind")]
    pub bind: SocketAddr,
    /// The `wss://` address agents dial, as printed in the enroll command.
    /// Default `wss://<hub name>:<bind port>`.
    #[serde(default)]
    pub public_url: Option<Url>,
    /// A `host:port` outside the network the hub checks before it blames a silent
    /// host. Default `1.1.1.1:53`.
    #[serde(default = "default_self_check")]
    pub self_check: String,
    /// How often agents send a heartbeat. Three missed heartbeats mean a silent agent.
    /// Default `10s`.
    #[serde(default = "default_agent_heartbeat")]
    pub heartbeat: Duration,
}

impl Default for AgentServerSection {
    fn default() -> Self {
        Self {
            bind: default_agent_bind(),
            public_url: None,
            self_check: default_self_check(),
            heartbeat: default_agent_heartbeat(),
        }
    }
}

/// `[incidents]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IncidentsSection {
    /// Consecutive failing runs before an incident opens. Default 2.
    #[serde(default = "default_confirm_runs")]
    pub confirm_runs: u8,
    /// A resolved incident that fails again within this window reopens. Default `"6h"`.
    #[serde(default = "default_reopen_window")]
    pub reopen_window: Duration,
    /// Signals on one host this close together are grouped. Default `"5m"`.
    #[serde(default = "default_grouping_window")]
    pub grouping_window: Duration,
    /// Incidents with no new signal for this long resolve. Optional.
    #[serde(default)]
    pub default_ttl: Option<Duration>,
}

impl Default for IncidentsSection {
    fn default() -> Self {
        Self {
            confirm_runs: default_confirm_runs(),
            reopen_window: default_reopen_window(),
            grouping_window: default_grouping_window(),
            default_ttl: None,
        }
    }
}

/// `[notify]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotifySection {
    /// Warnings wait until these hours end; critical alerts are always sent. Optional.
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
    /// How often an unacknowledged incident is announced again.
    #[serde(default)]
    pub renotify: Renotify,
    /// The daily digest.
    #[serde(default)]
    pub digest: Digest,
}

/// `quiet_hours = { .. }`. The range may cross midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    /// When quiet hours begin, such as `"00:00"`.
    pub start: LocalTime,
    /// When quiet hours end, such as `"09:00"`.
    pub end: LocalTime,
}

/// `renotify = { .. }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Renotify {
    /// Interval for warnings. Default `"24h"`.
    #[serde(default = "default_renotify_warning")]
    pub warning: Duration,
    /// Interval for critical incidents. Default `"4h"`.
    #[serde(default = "default_renotify_critical")]
    pub critical: Duration,
}

impl Default for Renotify {
    fn default() -> Self {
        Self {
            warning: default_renotify_warning(),
            critical: default_renotify_critical(),
        }
    }
}

/// `digest = { .. }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Digest {
    /// Whether the digest is sent. Default `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Local time it is sent. Default `"09:00"`.
    #[serde(default = "default_digest_time")]
    pub time: LocalTime,
}

impl Default for Digest {
    fn default() -> Self {
        Self {
            enabled: true,
            time: default_digest_time(),
        }
    }
}

/// `[discord]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscordSection {
    /// Whether the bot runs. Default `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The bot token.
    pub bot_token: Secret,
    /// The channel incidents are posted to.
    pub channel_id: u64,
    /// Discord users who may press buttons and run commands. Required when enabled.
    #[serde(default)]
    pub allowed_user_ids: Vec<u64>,
    /// A channel webhook used when the bot is down. Optional.
    #[serde(default)]
    pub webhook_url: Option<Secret>,
}

/// `[webhooks.<name>]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
    /// The payload format.
    pub kind: WebhookKind,
    /// How requests prove they are genuine.
    pub auth: WebhookAuth,
    /// The HMAC key or bearer token.
    pub secret: Secret,
    /// Host used when the payload names none. Optional.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::host_name")]
    pub default_host: Option<HostName>,
}

/// An inbound webhook payload format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookKind {
    /// Domovoi's own JSON format.
    Generic,
    /// Prometheus Alertmanager.
    Alertmanager,
    /// Uptime Kuma.
    UptimeKuma,
}

/// How an inbound webhook is authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookAuth {
    /// An HMAC-SHA256 signature of the body.
    Hmac,
    /// An `Authorization: Bearer` token.
    Bearer,
}

/// `[[probes]]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// The host the probe reports on.
    #[schemars(schema_with = "crate::schema::host_name")]
    pub host: HostName,
    /// What the probe does.
    pub kind: ProbeKind,
    /// `host:port` for `tcp`; an `http` or `https` URL for `http`.
    pub target: String,
    /// How often it runs. Default `"1m"`.
    #[serde(default = "default_minute")]
    pub interval: Duration,
    /// The HTTP status that counts as healthy. Required for `http`.
    #[serde(default)]
    pub expect_status: Option<u16>,
}

/// A network probe kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    /// Opens a TCP connection.
    Tcp,
    /// Sends an HTTP GET.
    Http,
}

/// `[heartbeat]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeartbeatSection {
    /// The URL pinged; it often embeds a token, so it is a secret.
    pub url: Secret,
    /// How often it is pinged. Default `"1m"`.
    #[serde(default = "default_minute")]
    pub interval: Duration,
}

/// `[backup]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupSection {
    /// Where backups are written. Default `<data_dir>/backups`.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Backups kept. Default 7.
    #[serde(default = "default_keep")]
    pub keep: u32,
}

impl Default for BackupSection {
    fn default() -> Self {
        Self {
            dir: None,
            keep: default_keep(),
        }
    }
}

/// `[secrets]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretsSection {
    /// Names of secrets you know were exposed; each raises an advisory until removed.
    #[serde(default)]
    pub exposed: Vec<String>,
}

impl HubConfig {
    /// The JSON Schema of `hub.toml`, for the reference documentation.
    #[must_use]
    pub fn json_schema() -> schemars::Schema {
        schemars::schema_for!(HubConfig)
    }
}

impl Secrets for HubConfig {
    fn secrets_mut(&mut self, visit: &mut dyn FnMut(&str, &mut Secret)) {
        for (slot, provider) in [
            ("primary", self.llm.primary.as_mut()),
            ("secondary", self.llm.secondary.as_mut()),
        ] {
            if let Some(key) = provider.and_then(|p| p.api_key.as_mut()) {
                visit(&format!("llm.{slot}.api_key"), key);
            }
        }
        if let Some(discord) = self.discord.as_mut() {
            visit("discord.bot_token", &mut discord.bot_token);
            if let Some(url) = discord.webhook_url.as_mut() {
                visit("discord.webhook_url", url);
            }
        }
        for (name, hook) in &mut self.webhooks {
            visit(&format!("webhooks.{name}.secret"), &mut hook.secret);
        }
        if let Some(heartbeat) = self.heartbeat.as_mut() {
            visit("heartbeat.url", &mut heartbeat.url);
        }
    }
}

fn default_hub_name() -> String {
    "domovoi".to_owned()
}
fn default_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/domovoi")
}
fn default_web_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 7420))
}
fn default_agent_bind() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 7421))
}
fn default_self_check() -> String {
    "1.1.1.1:53".to_owned()
}
fn default_agent_heartbeat() -> Duration {
    Duration::secs(10)
}
fn default_confirm_runs() -> u8 {
    2
}
fn default_reopen_window() -> Duration {
    Duration::secs(6 * 3600)
}
fn default_grouping_window() -> Duration {
    Duration::secs(5 * 60)
}
fn default_renotify_warning() -> Duration {
    Duration::secs(24 * 3600)
}
fn default_renotify_critical() -> Duration {
    Duration::secs(4 * 3600)
}
fn default_digest_time() -> LocalTime {
    LocalTime::hm(9, 0)
}
fn default_minute() -> Duration {
    Duration::secs(60)
}
fn default_keep() -> u32 {
    7
}
fn default_true() -> bool {
    true
}
