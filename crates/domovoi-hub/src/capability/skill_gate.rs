//! The skill switches of the registry: turning a skill on with its audit row, and why a host
//! cannot run skills at all.

use domovoi_core::SkillName;
use domovoi_core::capability::UnavailableReason;
use domovoi_core::{Host, HostId};

use super::registry::HubRegistry;

impl HubRegistry {
    /// Turns one skill on or off for `host`; `save` stores the settings value in the caller's
    /// commit. Memory changes only when `save` succeeds.
    pub async fn set_skill_enabled_with<F, Fut, E>(
        &self,
        host: &Host,
        skill: SkillName,
        enabled: bool,
        save: F,
    ) -> Result<(), E>
    where
        F: FnOnce(String, serde_json::Value) -> Fut,
        Fut: std::future::Future<Output = Result<(), E>>,
    {
        self.enablement.set_with(host, skill, enabled, save).await
    }

    /// Why skills cannot run on `host` at all (no usable skill runner), or `None` when they
    /// can or the host is not known to this hub right now. The rule the availability rows use.
    pub fn skills_cannot_run(&self, host: HostId) -> Option<UnavailableReason> {
        let words = self.agent.view(host)?.words;
        super::host_rules::runner_refusal(&words)
    }
}
