//! The skill manifest (`skills/<name>/skill.toml`) and the summary the hub sees.

use jiff::SignedDuration;

use super::ident_newtype;
use super::params::ParamSpec;
use crate::host::HostName;
use crate::sha256::sha256_newtype;
use crate::signal::CheckKey;

ident_newtype!(
    /// A skill name matching `^[a-z][a-z0-9_]{0,63}$`.
    SkillName,
    "skill name",
    64
);

sha256_newtype!(
    /// SHA-256 of a manifest's canonical JSON serialization.
    ///
    /// Approvals bind to it through the plan hash, so editing a manifest invalidates
    /// pending approvals.
    ManifestHash
);

/// What running a skill can do, ordered `Read < Write < Destructive`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Only reads state; usable by triage without approval.
    Read,
    /// Changes state reversibly; needs human approval.
    Write,
    /// Changes state irreversibly; needs typed confirmation in the web UI.
    Destructive,
}

/// How to check that a write skill worked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifySpec {
    /// Re-run the originating check (the only verification in v0.1).
    #[serde(default = "default_recheck")]
    pub recheck: bool,
    /// How long to wait before re-checking.
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub delay: SignedDuration,
}

fn default_recheck() -> bool {
    true
}

/// How to undo a write skill.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevertSpec {
    /// Script inside the skill folder that undoes the change.
    pub entry: String,
    /// How long after success a human may still ask for the revert.
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub confirm_window: SignedDuration,
}

/// A skill manifest as written in `skills/<name>/skill.toml` on the agent host.
///
/// Durations are friendly strings such as `"90s"` or `"10m"`. Parameter kinds are inline
/// tables tagged by `type`. Unknown keys are rejected so typos never pass silently.
///
/// ```
/// let text = r#"
/// name = "restart_container"
/// description = "Restart a container managed by compose"
/// risk = "write"
/// entry = "restart.sh"
/// timeout = "90s"
/// run_as = "domovoi-skills"
/// hosts = ["nas"]
/// applies_to = ["container.restart_loop"]
/// max_output_bytes = 65536
///
/// [[params]]
/// name = "container"
/// description = "Container name"
/// kind = { type = "string", pattern = "^[a-z0-9_.-]{1,64}$", max_len = 64 }
///
/// [[params]]
/// name = "wait_seconds"
/// description = "Seconds to wait before reporting"
/// required = false
/// kind = { type = "integer", min = 0, max = 300 }
///
/// [verify]
/// recheck = true
/// delay = "30s"
///
/// [revert]
/// entry = "start.sh"
/// confirm_window = "10m"
/// "#;
/// let manifest: domovoi_core::SkillManifest = toml::from_str(text).unwrap();
/// assert!(manifest.validate().is_ok());
/// assert_eq!(manifest.params.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    /// Unique name of the skill.
    pub name: SkillName,
    /// What the skill does, shown to humans and the model.
    pub description: String,
    /// What running it can change.
    pub risk: Risk,
    /// Path of the executable relative to the skill folder. Run directly, never through a shell.
    pub entry: String,
    /// Declared parameters, in argv order.
    #[serde(default)]
    pub params: Vec<ParamSpec>,
    /// Wall-clock limit, 1 second to 1 hour.
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub timeout: SignedDuration,
    /// System user to run as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_as: Option<String>,
    /// Hosts the skill may run on; empty means any.
    #[serde(default)]
    pub hosts: Vec<HostName>,
    /// Checks whose incidents the skill is relevant to.
    #[serde(default)]
    pub applies_to: Vec<CheckKey>,
    /// How to verify success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<VerifySpec>,
    /// How to undo the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert: Option<RevertSpec>,
    /// Cap on captured stdout and stderr, 1 KiB to 1 MiB.
    pub max_output_bytes: u32,
}

impl SkillManifest {
    /// SHA-256 over the canonical JSON serialization (fields in declaration order).
    #[must_use]
    pub fn manifest_hash(&self) -> ManifestHash {
        // Serializing this type cannot fail: every map key is a string and every field
        // serializer is infallible for valid values.
        let json = serde_json::to_vec(self).unwrap_or_default();
        ManifestHash::digest(&json)
    }

    /// What the hub may know about this manifest.
    #[must_use]
    pub fn summary(&self) -> SkillSummary {
        SkillSummary {
            name: self.name.clone(),
            description: self.description.clone(),
            risk: self.risk,
            params: self.params.clone(),
            applies_to: self.applies_to.clone(),
            manifest_hash: self.manifest_hash(),
        }
    }
}

/// The parts of a manifest an agent reports to the hub: no entry, user or limits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillSummary {
    /// Unique name of the skill.
    pub name: SkillName,
    /// What the skill does.
    pub description: String,
    /// What running it can change.
    pub risk: Risk,
    /// Declared parameters.
    pub params: Vec<ParamSpec>,
    /// Checks the skill is relevant to.
    pub applies_to: Vec<CheckKey>,
    /// Hash of the full manifest on the agent.
    pub manifest_hash: ManifestHash,
}
