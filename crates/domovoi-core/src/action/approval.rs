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
    /// The first 8 hex digits, for cards and logs. Not a security value.
    #[must_use]
    pub fn short(&self) -> String {
        hex::encode(&self.as_bytes()[..4])
    }

    /// SHA-256 over `"plan:v1\n{skill}\n{manifest_hash}\n{host_id}\n{canonical args}"`.
    ///
    /// The hash the hub sends with a skill run and the agent recomputes from its own copy of
    /// the skill, so it can refuse a run whose arguments or manifest differ from what the hub
    /// named. It is not the hash an approval binds; that is [`PlanHash::compute`].
    #[must_use]
    pub fn agent_check(
        skill: &SkillName,
        manifest: ManifestHash,
        host: HostId,
        args: &ValidatedArgs,
    ) -> Self {
        let text = format!("plan:v1\n{skill}\n{manifest}\n{host}\n{}", args.canonical());
        Self::digest(text.as_bytes())
    }
}

/// The plan hash an approval request says it is approving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanBinding {
    /// The whole hash (web).
    Full(PlanHash),
    /// The first 3 or more bytes of the hash (a chat button, whose custom ID has no room
    /// for more).
    Prefix(Vec<u8>),
}

impl PlanBinding {
    /// Shortest prefix accepted, in bytes.
    pub const MIN_PREFIX: usize = 3;

    /// Whether this binding names `stored`. A prefix shorter than [`Self::MIN_PREFIX`] or
    /// longer than 32 never matches.
    #[must_use]
    pub fn matches(&self, stored: &PlanHash) -> bool {
        match self {
            Self::Full(hash) => hash == stored,
            Self::Prefix(prefix) => {
                Self::MIN_PREFIX <= prefix.len()
                    && prefix.len() <= stored.as_bytes().len()
                    && stored.as_bytes().starts_with(prefix)
            }
        }
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

/// Where an approval was made: the authenticated surface and a non-reversible session
/// reference.
///
/// `session_ref` is never the session cookie or a token; it is a one-way reference the hub can
/// use to correlate its own records. `client` is `web` or `chat:<platform>`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ApprovalProvenance {
    session_ref: Option<String>,
    client: String,
}

impl ApprovalProvenance {
    /// Records the provenance of one approval.
    #[must_use]
    pub fn new(session_ref: Option<String>, client: impl Into<String>) -> Self {
        Self {
            session_ref,
            client: client.into(),
        }
    }

    /// The opaque, non-reversible session reference, when the surface had one.
    #[must_use]
    pub fn session_ref(&self) -> Option<&str> {
        self.session_ref.as_deref()
    }

    /// The surface: `web`, or `chat:<platform>`.
    #[must_use]
    pub fn client(&self) -> &str {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> PlanHash {
        hex::encode([byte; 32]).parse().unwrap()
    }

    #[test]
    fn plan_binding_full_matches_only_the_same_hash() {
        let stored = hash(0xab);
        assert!(PlanBinding::Full(stored).matches(&stored));
        assert!(!PlanBinding::Full(hash(0xac)).matches(&stored));
    }

    #[test]
    fn plan_binding_prefix_needs_at_least_three_bytes() {
        let stored = hash(0xab);
        let bytes = stored.as_bytes();
        assert!(PlanBinding::Prefix(bytes[..3].to_vec()).matches(&stored));
        assert!(PlanBinding::Prefix(bytes.to_vec()).matches(&stored));
        assert!(!PlanBinding::Prefix(bytes[..2].to_vec()).matches(&stored));
        assert!(!PlanBinding::Prefix([bytes.as_slice(), &[0u8]].concat()).matches(&stored));
        assert!(!PlanBinding::Prefix(vec![0, 0, 0]).matches(&stored));
    }

    #[test]
    fn plan_hash_short_is_the_first_eight_hex_digits() {
        let hash = hash(0x0a);
        assert_eq!(hash.short(), "0a0a0a0a");
        assert_eq!(hash.short(), &hash.to_string()[..8]);
    }
}
