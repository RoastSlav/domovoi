//! Plan hashes of package jobs: what exactly an owner's approval of an install, an uninstall
//! or a list binds.
//!
//! The encoding is length-prefixed like plan v2, under its own domain line, so a package hash
//! can never equal a skill hash. The execution id is deliberately not an input: a retry is a
//! new execution of the same plan and compares the same hash.

use super::approval::PlanHash;
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::HostId;
use crate::install::InstallKind;
use crate::package::{ArchiveDigest, KeyId, PackageName, PackageVersion};

/// The version of every package contract the hub calls.
const CONTRACT_VERSION: u32 = 1;

/// One host's package plan: the contract, what it applies to and the host it runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePlan {
    contract: &'static str,
    subject: String,
    target: String,
}

impl PackagePlan {
    /// The plan to install `name` `version` with archive `digest`, signed by `key_id`, on
    /// `host` whose enrolled agent key is `host_key`.
    #[must_use]
    pub fn install(
        name: &PackageName,
        version: &PackageVersion,
        digest: &ArchiveDigest,
        key_id: &KeyId,
        host: HostId,
        host_key: &KeyFingerprint,
    ) -> Self {
        Self {
            contract: "agent.package.install",
            subject: format!("name={name}\nversion={version}\nsha256={digest}\nkey={key_id}"),
            target: host_target_text(host, host_key),
        }
    }

    /// The plan to remove `name` `version` from `host`.
    #[must_use]
    pub fn uninstall(
        name: &PackageName,
        version: &PackageVersion,
        host: HostId,
        host_key: &KeyFingerprint,
    ) -> Self {
        Self {
            contract: "agent.package.uninstall",
            subject: format!("name={name}\nversion={version}"),
            target: host_target_text(host, host_key),
        }
    }

    /// The plan to let the skills of the stored `name` `version` run on `host`.
    #[must_use]
    pub fn enable(
        name: &PackageName,
        version: &PackageVersion,
        host: HostId,
        host_key: &KeyFingerprint,
    ) -> Self {
        Self {
            contract: "agent.package.enable",
            subject: format!("name={name}\nversion={version}"),
            target: host_target_text(host, host_key),
        }
    }

    /// The plan to stop the skills of the stored `name` `version` from running on `host`.
    #[must_use]
    pub fn disable(
        name: &PackageName,
        version: &PackageVersion,
        host: HostId,
        host_key: &KeyFingerprint,
    ) -> Self {
        Self {
            contract: "agent.package.disable",
            subject: format!("name={name}\nversion={version}"),
            target: host_target_text(host, host_key),
        }
    }

    /// The plan to list what is installed on `host`.
    #[must_use]
    pub fn list(host: HostId, host_key: &KeyFingerprint) -> Self {
        Self {
            contract: "agent.package.list",
            subject: "none".to_owned(),
            target: host_target_text(host, host_key),
        }
    }

    /// The exact text that is hashed.
    #[must_use]
    pub fn canonical(&self) -> String {
        let contract = format!("{}@{CONTRACT_VERSION}", self.contract);
        format!(
            "plan:package-v1\ncontract:{}:{contract}\nsubject:{}:{}\ntarget:{}:{}\n",
            contract.len(),
            self.subject.len(),
            self.subject,
            self.target.len(),
            self.target,
        )
    }

    /// The plan hash an owner approves for this host.
    #[must_use]
    pub fn hash(&self) -> PlanHash {
        PlanHash::digest(self.canonical().as_bytes())
    }
}

/// The hash over every host's plan in one job, so one confirmation covers the whole job.
///
/// The per-host hashes are sorted, so the order the hosts were named in does not matter, and
/// the kind is part of the input, so an install job and an uninstall job over the same hashes
/// differ.
#[must_use]
pub fn job_plan_hash(kind: InstallKind, hosts: &[PlanHash]) -> PlanHash {
    let mut sorted: Vec<[u8; 32]> = hosts.iter().map(|hash| *hash.as_bytes()).collect();
    sorted.sort_unstable();
    let mut text = format!("plan:package-job-v1\nkind:{}\n", kind.as_str());
    for hash in sorted {
        text.push_str(&hex::encode(hash));
        text.push('\n');
    }
    PlanHash::digest(text.as_bytes())
}

#[cfg(test)]
mod tests;
