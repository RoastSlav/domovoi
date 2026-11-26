//! The plan hash of one block: what an owner's approval of containment binds.
//!
//! Length-prefixed like plan v2, under its own domain line. The phase and the execution id are
//! deliberately not inputs, so `apply`, `confirm` and `rollback` of one block carry one hash
//! and one approval covers exactly one block. The safety identity, or the reason it is
//! unverified, is an input: the approval binds what it showed.

use jiff::SignedDuration;

use super::approval::PlanHash;
use crate::attest::{SafetyIdentity, UnverifiedReason};
use crate::containment::Net;
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::HostId;

/// What the approval binds about safety.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAssurance {
    /// The verified identity of the helper at approval time.
    Verified(SafetyIdentity),
    /// The reason coverage could not be verified.
    Unverified(UnverifiedReason),
}

impl PlanAssurance {
    fn text(&self) -> String {
        match self {
            Self::Verified(id) => format!(
                "verified:{}/{}/{}/{}/{}",
                id.fingerprint, id.boot_id, id.generation, id.config_hash, id.route_state_hash
            ),
            Self::Unverified(reason) => format!("unverified:{}", reason.token()),
        }
    }
}

/// One block's plan: the contract, the source, the TTL, the host and the assurance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainmentPlan {
    source: String,
    ttl_secs: i64,
    target: String,
    assurance: String,
}

impl ContainmentPlan {
    /// The plan to block `net` for `ttl` on `host` whose enrolled key is `host_key`.
    #[must_use]
    pub fn new(
        net: &Net,
        ttl: SignedDuration,
        host: HostId,
        host_key: &KeyFingerprint,
        assurance: &PlanAssurance,
    ) -> Self {
        Self {
            source: net.canonical(),
            ttl_secs: ttl.as_secs(),
            target: host_target_text(host, host_key),
            assurance: assurance.text(),
        }
    }

    /// The exact text that is hashed.
    #[must_use]
    pub fn canonical(&self) -> String {
        let contract = "firewall.block_source@1";
        format!(
            "plan:containment-v1\ncontract:{}:{contract}\nsource:{}:{}\nttl:{}\ntarget:{}:{}\nassurance:{}:{}\n",
            contract.len(),
            self.source.len(),
            self.source,
            self.ttl_secs,
            self.target.len(),
            self.target,
            self.assurance.len(),
            self.assurance,
        )
    }

    /// The plan hash an owner approves.
    #[must_use]
    pub fn hash(&self) -> PlanHash {
        PlanHash::digest(self.canonical().as_bytes())
    }
}
