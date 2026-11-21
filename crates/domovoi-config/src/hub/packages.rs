//! `[packages]` in `hub.toml`: the hub's package store and the keys that sign what it holds.
//! The same table name also holds the install gate in `agent.toml`; the two files read
//! different keys from it (see `env::AGENT_SECTIONS`).

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::duration::Duration;
use crate::issue::ConfigIssue;
use crate::packages::TrustedKey;
use crate::validate_packages::{key_list_rules, revoked_rules};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const GIB: u64 = 1024 * MIB;

/// `[packages]`. Off by default: with `enabled = false` the hub imports and installs nothing,
/// though it still lists what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct PackagesHubSection {
    /// Whether packages can be imported and installed from this hub. Default `false`.
    pub enabled: bool,
    /// Largest archive accepted on import, in bytes. 1 KiB to 256 MiB; default 20 MiB.
    pub max_artifact_bytes: u64,
    /// Most bytes of archives the hub keeps. 1 MiB to 16 GiB; default 512 MiB.
    pub max_store_bytes: u64,
    /// Most archives the hub keeps. 1 to 1000; default 64.
    pub max_artifacts: u32,
    /// How long a person's approval to install stays valid. 1 minute to 1 hour; default
    /// `"10m"`.
    pub approval_window: Duration,
    /// Days finished install jobs are kept. 7 to 3650; default 180.
    pub job_history_days: u32,
    /// Key ids (16 lowercase hex characters) whose packages are never accepted, even when
    /// also listed in `owner_keys`. Read from the file only; the web UI cannot change it.
    pub revoked: Vec<String>,
    /// Keys whose signatures the hub accepts on import. Read from the file only; the web UI
    /// cannot change them.
    pub owner_keys: Vec<TrustedKey>,
}

impl Default for PackagesHubSection {
    fn default() -> Self {
        Self {
            enabled: false,
            max_artifact_bytes: 20 * MIB,
            max_store_bytes: 512 * MIB,
            max_artifacts: 64,
            approval_window: Duration::secs(600),
            job_history_days: 180,
            revoked: Vec::new(),
            owner_keys: Vec::new(),
        }
    }
}

fn out_of_range(issues: &mut Vec<ConfigIssue>, field: &str, range: &str) {
    issues.push(ConfigIssue::error(
        format!("packages.{field}"),
        format!("must be {range}"),
    ));
}

impl PackagesHubSection {
    /// Pushes every problem in this section onto `issues`.
    pub(crate) fn validate(&self, issues: &mut Vec<ConfigIssue>) {
        if !(KIB..=256 * MIB).contains(&self.max_artifact_bytes) {
            out_of_range(issues, "max_artifact_bytes", "1 KiB to 256 MiB");
        }
        if !(MIB..=16 * GIB).contains(&self.max_store_bytes) {
            out_of_range(issues, "max_store_bytes", "1 MiB to 16 GiB");
        }
        if !(1..=1000).contains(&self.max_artifacts) {
            out_of_range(issues, "max_artifacts", "1 to 1000");
        }
        if !(60..=3600).contains(&self.approval_window.as_signed().as_secs()) {
            out_of_range(issues, "approval_window", "1 minute to 1 hour");
        }
        if !(7..=3650).contains(&self.job_history_days) {
            out_of_range(issues, "job_history_days", "7 to 3650 days");
        }
        let ids: BTreeSet<&str> = key_list_rules("packages.owner_keys", &self.owner_keys, issues);
        revoked_rules("packages.revoked", &self.revoked, &ids, issues);
        if self.enabled && self.owner_keys.is_empty() {
            issues.push(ConfigIssue::warning(
                "packages.owner_keys",
                "packages are enabled but no owner_keys are set, so no package can be imported",
            ));
        }
    }
}
