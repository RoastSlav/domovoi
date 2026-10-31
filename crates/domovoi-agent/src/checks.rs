//! Which checks the agent runs, and their settings from `agent.toml`.

use std::collections::BTreeMap;

use domovoi_checks::framework::{Check, CheckSettings, Registry};
use domovoi_config::AgentConfig;
use domovoi_core::CheckKey;

/// A set of checks the scheduler and `run_check` draw from.
pub trait CheckSet: Send + Sync {
    /// Every check, in key order.
    fn all(&self) -> Vec<&dyn Check>;
    /// The check with `key`.
    fn get(&self, key: &CheckKey) -> Option<&dyn Check>;
}

impl CheckSet for Registry {
    fn all(&self) -> Vec<&dyn Check> {
        self.checks().collect()
    }

    fn get(&self, key: &CheckKey) -> Option<&dyn Check> {
        Registry::get(self, key)
    }
}

impl CheckSet for Vec<Box<dyn Check>> {
    fn all(&self) -> Vec<&dyn Check> {
        self.iter().map(AsRef::as_ref).collect()
    }

    fn get(&self, key: &CheckKey) -> Option<&dyn Check> {
        self.iter().map(AsRef::as_ref).find(|c| &c.key() == key)
    }
}

/// The settings for every check named under `[checks]`.
pub fn check_settings(config: &AgentConfig) -> Result<BTreeMap<CheckKey, CheckSettings>, String> {
    config
        .checks
        .iter()
        .map(|(key, check)| {
            let thresholds = match serde_json::to_value(&check.thresholds) {
                Ok(serde_json::Value::Object(map)) => map,
                Ok(_) => serde_json::Map::new(),
                Err(err) => return Err(format!("[checks.{key}] thresholds: {err}")),
            };
            let settings = CheckSettings {
                enabled: check.enabled,
                cadence: check.interval.map(|i| i.as_signed()),
                thresholds,
                criticality: check.criticality,
            };
            Ok((key.clone(), settings))
        })
        .collect()
}
