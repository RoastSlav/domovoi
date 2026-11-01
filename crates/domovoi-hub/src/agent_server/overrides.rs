//! Per-host check overrides from the web UI: sent in `welcome` and pushed as
//! `config_update` when they change (ADR 0017).

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::SettingsRepo;
use domovoi_core::{CheckKey, HostId};
use domovoi_proto::Message;
use domovoi_proto::messages::{AgentSettings, CheckInfo, CheckOverride, ConfigUpdate};

use super::{AgentServer, Inner};

/// The overrides stored for the host called `name`; none when nothing is stored or the
/// stored value is unreadable.
pub(crate) async fn stored_overrides(
    repo: &dyn SettingsRepo,
    name: &str,
) -> BTreeMap<CheckKey, CheckOverride> {
    let key = crate::settings::host_checks_key(name);
    match repo.get(&key).await {
        Ok(Some(value)) => serde_json::from_value(value).unwrap_or_else(|err| {
            tracing::warn!(%err, host = name, "ignoring unreadable check overrides");
            BTreeMap::new()
        }),
        Ok(None) => BTreeMap::new(),
        Err(err) => {
            tracing::warn!(%err, host = name, "could not read check overrides");
            BTreeMap::new()
        }
    }
}

/// The settings to send the agent of `host`.
pub(crate) async fn agent_settings(inner: &Inner, host: HostId) -> AgentSettings {
    let heartbeat_seconds = u32::try_from(inner.settings.heartbeat.as_secs()).unwrap_or(u32::MAX);
    let mut settings = AgentSettings {
        heartbeat_seconds,
        check_overrides: BTreeMap::new(),
    };
    let Some(repo) = inner.overrides.get() else {
        return settings;
    };
    if let Ok(Some(found)) = inner.deps.hosts.get_host(host).await {
        settings.check_overrides = stored_overrides(&**repo, found.name.as_str()).await;
    }
    settings
}

impl AgentServer {
    /// Reads per-host check overrides from `repo` for every handshake from now on.
    /// Without it, agents get no overrides.
    pub fn use_check_overrides(&self, repo: Arc<dyn SettingsRepo>) {
        let _ = self.inner.overrides.set(repo);
    }

    /// The checks the host's agent reported when it connected; `None` while offline.
    #[must_use]
    pub fn reported_checks(&self, host: HostId) -> Option<Vec<CheckInfo>> {
        self.inner.registry.checks(host)
    }

    /// Sends the host's current settings, overrides included, to its live agent. False
    /// when the agent is offline; it gets them at its next handshake.
    pub async fn push_settings(&self, host: HostId) -> bool {
        let config = agent_settings(&self.inner, host).await;
        self.inner
            .registry
            .push(host, &Message::ConfigUpdate(ConfigUpdate { config }))
    }
}
