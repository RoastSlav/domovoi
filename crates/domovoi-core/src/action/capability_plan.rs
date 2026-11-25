//! The plan hash of a built-in helper capability: what exactly an approval or a permit of a
//! unit restart binds.
//!
//! A new domain-separated family, never `plan:v1` or `plan:v2`: v2 needs a skill manifest and a
//! check key, and the unit's identity hash must be bound. The literal `plan:capability-v1` is
//! written only in this file.
//!
//! Bound: contract id and version, host id and enrolled key, the target identity, the identity
//! hash (when there is one), the canonical arguments and the execution id. Not bound: the
//! incident, the proposer, the expiry, the approval time, UI text, the helper or agent binary.

use std::collections::BTreeMap;

use super::approval::PlanHash;
use crate::capability::TargetIdentity;
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::{ExecutionId, HostId};
use crate::unit::{IdentityHash, UnitName};

/// One helper capability's plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityPlan {
    contract: &'static str,
    version: u32,
    host: String,
    target: Option<String>,
    identity_hash: Option<String>,
    args: BTreeMap<String, String>,
    execution: ExecutionId,
}

impl CapabilityPlan {
    /// The plan to restart the unit `target` on `host`, bound to `identity_hash`.
    #[must_use]
    pub fn unit_restart(
        host: HostId,
        host_key: &KeyFingerprint,
        target: &TargetIdentity,
        identity_hash: &IdentityHash,
        args: &BTreeMap<String, String>,
        execution: ExecutionId,
    ) -> Self {
        Self {
            contract: "unit.restart",
            version: 1,
            host: host_target_text(host, host_key),
            target: Some(target.to_string()),
            identity_hash: Some(identity_hash.to_string()),
            args: args.clone(),
            execution,
        }
    }

    /// The plan to read the identity of the unit `name` on `host`.
    #[must_use]
    pub fn unit_identity_read(
        host: HostId,
        host_key: &KeyFingerprint,
        name: &UnitName,
        execution: ExecutionId,
    ) -> Self {
        let mut args = BTreeMap::new();
        args.insert("unit".to_owned(), name.to_string());
        Self {
            contract: "unit.identity",
            version: 1,
            host: host_target_text(host, host_key),
            target: None,
            identity_hash: None,
            args,
            execution,
        }
    }

    /// The exact text that is hashed.
    #[must_use]
    pub fn canonical(&self) -> String {
        fn framed(name: &str, value: &str) -> String {
            format!("{name}:{}:{value}\n", value.len())
        }
        let contract = format!("{}@{}", self.contract, self.version);
        let args: String = self.args.iter().map(|(k, v)| framed(k, v)).collect();
        let mut text = String::from("plan:capability-v1\n");
        text.push_str(&framed("contract", &contract));
        text.push_str(&framed("host", &self.host));
        text.push_str(&framed(
            "target",
            self.target.as_deref().unwrap_or("absent"),
        ));
        text.push_str(&framed(
            "identity_hash",
            self.identity_hash.as_deref().unwrap_or("absent"),
        ));
        text.push_str(&framed("args", &args));
        text.push_str(&format!("execution:{}\n", self.execution));
        text
    }

    /// The plan hash.
    #[must_use]
    pub fn hash(&self) -> PlanHash {
        PlanHash::digest(self.canonical().as_bytes())
    }
}
