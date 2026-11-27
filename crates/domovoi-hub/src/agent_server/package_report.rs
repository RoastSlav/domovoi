//! The package skills of each host, kept so they can still be shown while its agent is away.
//!
//! The agent decides what is enabled and says so in `hello`; the hub stores the last answer
//! for display. Nothing is enabled from this copy, so restoring or editing it changes only
//! what a page shows. A skill read from it is never offered as available.

use domovoi_core::HostId;
use domovoi_core::ports::SettingsRepo;
use domovoi_proto::messages::PackageSkillState;

/// A host's package skills and where the answer came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageReport {
    /// Every skill of a stored package, on or off.
    pub skills: Vec<PackageSkillState>,
    /// Whether the agent is connected and this is what it announced. `false` for the stored
    /// copy of an earlier connection.
    pub live: bool,
}

fn key(host: HostId) -> String {
    format!("package_report.{host}")
}

/// Stores what `host`'s agent last said.
pub(super) async fn save(store: &dyn SettingsRepo, host: HostId, skills: &[PackageSkillState]) {
    let written = match serde_json::to_value(skills) {
        Ok(value) => store
            .set(&key(host), &value)
            .await
            .map_err(|e| e.to_string()),
        Err(err) => Err(err.to_string()),
    };
    if let Err(err) = written {
        tracing::warn!(%host, %err, "could not store the host's package skills");
    }
}

/// What `host`'s agent said when it was last connected; empty when it never said.
pub(super) async fn load(store: &dyn SettingsRepo, host: HostId) -> Vec<PackageSkillState> {
    match store.get(&key(host)).await {
        Ok(found) => found
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default(),
        Err(err) => {
            tracing::warn!(%host, %err, "could not read the host's stored package skills");
            Vec::new()
        }
    }
}

/// Removes the stored copy, when the host's agent is revoked.
pub(super) async fn forget(store: &dyn SettingsRepo, host: HostId) {
    if let Err(err) = store.set_many(&[(key(host), None)]).await {
        tracing::warn!(%host, %err, "could not remove the host's stored package skills");
    }
}
