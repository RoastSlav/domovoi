//! What the agent server needs from the hub configuration.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use domovoi_config::hub::{HubConfig, ProbeKind};
use domovoi_core::HostName;
use jiff::SignedDuration;
use url::Url;

use super::error::AgentServerError;

/// Settings for the agent server, taken from `[hub]`, `[agent_server]` and `[[probes]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettings {
    /// The hub's name, used in the certificate.
    pub hub_name: String,
    /// The directory the TLS identity is kept under (`<data_dir>/tls`).
    pub data_dir: PathBuf,
    /// Where agents connect.
    pub bind: SocketAddr,
    /// The address agents dial, printed in the enroll command.
    pub public_url: Url,
    /// The `host:port` outside the network checked before blaming a silent host.
    pub self_check: String,
    /// How often agents send a heartbeat.
    pub heartbeat: SignedDuration,
    /// `host:port` TCP probe targets configured per host, used to tell host down from
    /// agent down.
    pub probes: BTreeMap<HostName, Vec<String>>,
}

impl ServerSettings {
    /// Reads the settings from `config`. The public URL defaults to
    /// `wss://<hub name>:<bind port>`.
    pub fn from_config(config: &HubConfig) -> Result<Self, AgentServerError> {
        let server = &config.agent_server;
        let public_url = match &server.public_url {
            Some(url) => url.clone(),
            None => {
                let text = format!("wss://{}:{}", config.hub.name, server.bind.port());
                Url::parse(&text).map_err(|e| {
                    AgentServerError::Settings(format!(
                        "{text:?} is not a usable URL ({e}); set [agent_server] public_url"
                    ))
                })?
            }
        };
        let mut probes: BTreeMap<HostName, Vec<String>> = BTreeMap::new();
        for probe in config.probes.iter().filter(|p| p.kind == ProbeKind::Tcp) {
            probes
                .entry(probe.host.clone())
                .or_default()
                .push(probe.target.clone());
        }
        Ok(Self {
            hub_name: config.hub.name.clone(),
            data_dir: config.hub.data_dir.clone(),
            bind: server.bind,
            public_url,
            self_check: server.self_check.clone(),
            heartbeat: server.heartbeat.as_signed(),
            probes,
        })
    }

    /// How long an agent may stay quiet before the hub looks into it: three heartbeats.
    #[must_use]
    pub fn silence(&self) -> SignedDuration {
        self.heartbeat.saturating_mul(3)
    }
}
