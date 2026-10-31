//! Plan hashes and the value that carries a human approval.

use jiff::Timestamp;

use crate::id::{HostId, UserId};
use crate::sha256::sha256_newtype;
use crate::skill::{ManifestHash, SkillName, ValidatedArgs};

sha256_newtype!(
    /// What exactly an approval authorises: skill, manifest version, host and arguments.
    ///
    /// Recomputed at dispatch; any difference rejects the action (INV-4).
    PlanHash
);

impl PlanHash {
    /// SHA-256 over `"plan:v1\n{skill}\n{manifest_hash}\n{host_id}\n{canonical args}"`.
    #[must_use]
    pub fn compute(
        skill: &SkillName,
        manifest: ManifestHash,
        host: HostId,
        args: &ValidatedArgs,
    ) -> Self {
        let text = format!("plan:v1\n{skill}\n{manifest}\n{host}\n{}", args.canonical());
        Self::digest(text.as_bytes())
    }
}

/// Where a human approved an action.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalChannel {
    /// The web UI, with a verified session and CSRF token.
    Web {
        /// Whether the human typed the confirmation phrase (required for destructive skills).
        typed_confirmation: bool,
    },
    /// A chat button pressed by an allowlisted user.
    Chat {
        /// Chat platform, for example `discord`.
        platform: String,
        /// The platform's user id.
        platform_user: String,
    },
}

/// Proof that an authenticated human approved an action (INV-1).
///
/// There is deliberately no `Default` and no `Deserialize`: the only ways to obtain one are
/// [`HumanApproval::new`], after authentication, and [`HumanApproval::restore`], when the
/// store loads an approval it persisted (ADR 0014).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HumanApproval {
    user: UserId,
    channel: ApprovalChannel,
    at: Timestamp,
}

impl HumanApproval {
    /// Records that `user` approved through `channel` at `at`.
    ///
    /// Construct only in `crates/domovoi-hub/src/auth/` after authentication.
    /// `xtask arch-check` enforces this (INV-1).
    #[must_use]
    pub fn new(user: UserId, channel: ApprovalChannel, at: Timestamp) -> Self {
        Self { user, channel, at }
    }

    /// Rebuilds an approval the store persisted when [`HumanApproval::new`] created it.
    ///
    /// Call only in `crates/domovoi-store/src/` when loading a stored action. It records
    /// no new decision. `xtask arch-check` enforces this (INV-1, ADR 0014).
    #[must_use]
    pub fn restore(user: UserId, channel: ApprovalChannel, at: Timestamp) -> Self {
        Self { user, channel, at }
    }

    /// Who approved.
    #[must_use]
    pub fn user(&self) -> UserId {
        self.user
    }

    /// Where they approved.
    #[must_use]
    pub fn channel(&self) -> &ApprovalChannel {
        &self.channel
    }

    /// When they approved.
    #[must_use]
    pub fn at(&self) -> Timestamp {
        self.at
    }
}
