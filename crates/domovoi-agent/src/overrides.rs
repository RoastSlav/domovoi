//! Check settings managed from the hub (ADR 0017): the hub sends per-check overrides in
//! `welcome` and `config_update`, and they apply live over the local `agent.toml`
//! values. Turning a check on or off, its interval and its thresholds change without a
//! restart; the next `config_update` replaces the previous overrides entirely.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use domovoi_checks::framework::CheckSettings;
use domovoi_core::CheckKey;
use domovoi_proto::messages::{AgentSettings, CheckOverride};
use jiff::SignedDuration;
use tokio::sync::watch;

use crate::docker::DockerControl;

/// The checks that read the container runtime; they stop while it is switched off.
const CONTAINER_CHECKS: [&str; 4] = [
    "container.expected_down",
    "container.unhealthy",
    "container.restart_loop",
    "container.new",
];

/// Check settings in effect, by key. A check without an entry uses its defaults.
pub type SettingsMap = BTreeMap<CheckKey, CheckSettings>;

/// A read-only view of the settings in effect, which changes when the hub sends new
/// overrides.
pub type SettingsView = watch::Receiver<Arc<SettingsMap>>;

/// A view that never changes, for tests and one-off runs.
#[must_use]
pub fn fixed(settings: SettingsMap) -> SettingsView {
    watch::channel(Arc::new(settings)).1
}

/// The local settings and the overrides on top of them.
#[derive(Clone)]
pub struct EffectiveChecks {
    base: Arc<SettingsMap>,
    current: Arc<watch::Sender<Arc<SettingsMap>>>,
    overrides: Arc<Mutex<BTreeMap<CheckKey, CheckOverride>>>,
    docker: Option<DockerControl>,
}

impl std::fmt::Debug for EffectiveChecks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectiveChecks")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl EffectiveChecks {
    /// Starts with the local settings and no overrides.
    #[must_use]
    pub fn new(base: SettingsMap) -> Self {
        let base = Arc::new(base);
        Self {
            current: Arc::new(watch::Sender::new(base.clone())),
            base,
            overrides: Arc::default(),
            docker: None,
        }
    }

    /// Also applies the hub's container runtime choice, and keeps the container checks
    /// off while the runtime is.
    #[must_use]
    pub fn with_docker(mut self, docker: DockerControl) -> Self {
        self.docker = Some(docker);
        self.recompute();
        self
    }

    /// Applies everything the hub sent: the container runtime choice and the overrides.
    pub fn apply_settings(&self, settings: &AgentSettings) {
        if let Some(docker) = &self.docker {
            docker.choose(settings.docker);
        }
        self.apply(&settings.check_overrides);
    }

    /// The settings in effect, and every later change.
    #[must_use]
    pub fn view(&self) -> SettingsView {
        self.current.subscribe()
    }

    /// Replaces the overrides. Readers are woken only when the result changed.
    pub fn apply(&self, overrides: &BTreeMap<CheckKey, CheckOverride>) {
        *self
            .overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = overrides.clone();
        self.recompute();
        if !overrides.is_empty() {
            tracing::info!(
                checks = overrides.len(),
                "applied check overrides from the hub"
            );
        }
    }
}

impl EffectiveChecks {
    /// The settings in effect from the local ones, the hub's overrides and the container
    /// runtime switch. Readers are woken only when the result changed.
    fn recompute(&self) {
        let overrides = self
            .overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut next = merge(&self.base, &overrides);
        if self.docker.as_ref().is_some_and(|d| !d.is_on()) {
            for key in CONTAINER_CHECKS
                .iter()
                .filter_map(|k| CheckKey::parse(k).ok())
            {
                next.entry(key).or_default().enabled = false;
            }
        }
        self.current.send_if_modified(|current| {
            if **current == next {
                false
            } else {
                *current = Arc::new(next);
                true
            }
        });
    }
}

/// `base` with `overrides` on top: each set field replaces the local one, and
/// thresholds merge key by key.
#[must_use]
pub fn merge(base: &SettingsMap, overrides: &BTreeMap<CheckKey, CheckOverride>) -> SettingsMap {
    let mut out = base.clone();
    for (key, change) in overrides {
        let settings = out.entry(key.clone()).or_default();
        if let Some(enabled) = change.enabled {
            settings.enabled = enabled;
        }
        if let Some(seconds) = change.interval_seconds.filter(|s| *s > 0) {
            settings.cadence = Some(SignedDuration::from_secs(i64::from(seconds)));
        }
        for (name, value) in &change.thresholds {
            settings.thresholds.insert(name.clone(), value.clone());
        }
    }
    out
}

/// What `hello` reports for every check in `registry`: its local settings, before any
/// override.
#[must_use]
pub fn report(
    registry: &domovoi_checks::framework::Registry,
    base: &SettingsMap,
) -> Vec<domovoi_proto::messages::CheckInfo> {
    registry
        .checks()
        .map(|check| {
            let key = check.key();
            let local = base.get(&key).cloned().unwrap_or_default();
            let interval = local.cadence.unwrap_or_else(|| check.default_cadence());
            domovoi_proto::messages::CheckInfo {
                key,
                enabled: local.enabled,
                interval_seconds: u32::try_from(interval.as_secs().max(1)).unwrap_or(u32::MAX),
                thresholds: local.thresholds.into_iter().collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domovoi_checks::framework::CheckSettings;
    use domovoi_core::CheckKey;
    use domovoi_proto::messages::CheckOverride;
    use serde_json::json;

    use super::{EffectiveChecks, merge};

    #[test]
    fn overrides_replace_fields_and_merge_thresholds() {
        let disk = CheckKey::parse("disk.usage").unwrap();
        let mut base = BTreeMap::new();
        base.insert(
            disk.clone(),
            CheckSettings::with_thresholds(json!({ "warn_percent": 80, "crit_percent": 95 })),
        );
        let overrides = BTreeMap::from([(
            disk.clone(),
            CheckOverride {
                enabled: Some(false),
                interval_seconds: Some(120),
                thresholds: BTreeMap::from([("warn_percent".to_owned(), json!(70))]),
            },
        )]);
        let merged = merge(&base, &overrides);
        let disk_settings = &merged[&disk];
        assert!(!disk_settings.enabled);
        assert_eq!(disk_settings.cadence.unwrap().as_secs(), 120);
        assert_eq!(disk_settings.thresholds["warn_percent"], json!(70));
        assert_eq!(disk_settings.thresholds["crit_percent"], json!(95));

        let effective = EffectiveChecks::new(base);
        let mut view = effective.view();
        effective.apply(&overrides);
        assert!(view.has_changed().unwrap());
        assert!(!view.borrow_and_update()[&disk].enabled);
        effective.apply(&BTreeMap::new());
        assert!(
            view.borrow_and_update()[&disk].enabled,
            "a new update replaces the old one"
        );
    }

    #[test]
    fn container_checks_follow_the_runtime_switch_the_hub_flips() {
        use domovoi_checks::containers::Detection;
        use domovoi_config::DockerMode;
        use domovoi_proto::messages::AgentSettings;

        use crate::docker::DockerControl;

        let detection = Detection::Found {
            endpoint: "unix:///run/podman/podman.sock".to_owned(),
            engine: "Podman",
        };
        let control = DockerControl::new(DockerMode::Auto, Some(&detection));
        let effective = EffectiveChecks::new(BTreeMap::new()).with_docker(control.clone());
        let view = effective.view();
        let new = CheckKey::parse("container.new").unwrap();
        let on = |view: &super::SettingsView| view.borrow().get(&new).is_none_or(|s| s.enabled);
        assert!(on(&view), "an engine that answers turns the checks on");

        let settings = |docker| AgentSettings {
            heartbeat_seconds: 10,
            check_overrides: BTreeMap::new(),
            docker,
        };
        effective.apply_settings(&settings(Some(false)));
        assert!(!control.is_on());
        assert!(!on(&view), "the hub switched the runtime off");
        effective.apply_settings(&settings(None));
        assert!(on(&view), "no choice goes back to what was found");
    }
}
