//! A host's check overrides: what its agent reported and what the UI saved over it.

use std::collections::BTreeMap;

use domovoi_config::ConfigIssue;
use domovoi_core::ports::SettingsRepo;
use domovoi_core::{CheckKey, Host, User};
use domovoi_proto::messages::{CheckInfo, CheckOverride};
use serde_json::json;

use super::service::{SettingsError, SettingsService};
use super::stored::host_checks_key;

/// A host's checks: what its agent reported and what the UI overrides.
#[derive(Debug, Clone, PartialEq)]
pub struct HostChecks {
    /// Whether the agent is connected now.
    pub connected: bool,
    /// What the agent reported at its last handshake, while it is connected.
    pub reported: Vec<CheckInfo>,
    /// The overrides saved for the host.
    pub overrides: BTreeMap<CheckKey, CheckOverride>,
}

impl SettingsService {
    /// A host's reported checks and saved overrides.
    pub async fn host_checks(&self, host: &Host) -> Result<HostChecks, SettingsError> {
        let deps = self.deps();
        let reported = deps.agents.reported_checks(host.id);
        Ok(HostChecks {
            connected: reported.is_some(),
            reported: reported.unwrap_or_default(),
            overrides: crate::agent_server::stored_overrides(&*deps.store, host.name.as_str())
                .await,
        })
    }

    /// Saves a host's check overrides and sends them to its agent. Returns whether the
    /// agent got them now; otherwise it gets them at its next handshake. No restart.
    pub async fn set_host_checks(
        &self,
        user: &User,
        host: &Host,
        overrides: BTreeMap<CheckKey, CheckOverride>,
    ) -> Result<bool, SettingsError> {
        let mut issues = Vec::new();
        for (key, item) in &overrides {
            if item.interval_seconds == Some(0) {
                issues.push(ConfigIssue::error(
                    format!("{key}.interval_seconds"),
                    "must be at least 1 second",
                ));
            }
        }
        if !issues.is_empty() {
            return Err(SettingsError::Invalid(issues));
        }
        let deps = self.deps();
        let value = serde_json::to_value(&overrides)
            .map_err(|e| SettingsError::Unavailable(e.to_string()))?;
        SettingsRepo::set(&*deps.store, &host_checks_key(host.name.as_str()), &value).await?;
        self.audit(
            user,
            "settings.host_checks",
            &format!("host:{}", host.name),
            json!({ "overrides": value }),
        )
        .await;
        Ok(deps.agents.push_settings(host.id).await)
    }
}
