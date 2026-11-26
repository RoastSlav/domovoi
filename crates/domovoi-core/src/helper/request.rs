//! The helper's request enum: closed, with no free-form field.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use super::block::BlockSource;
use super::group::HelperGroup;
use super::read::{ContainerRef, DeviceRef};
use crate::attest::ExpectedSafety;
use crate::unit::{IdentityHash, UnitId, UnitName};

/// A challenge for `safety.attest`: 32 random bytes, written as 64 lowercase hex characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Nonce([u8; 32]);

/// Why text is not a nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a nonce is 64 lowercase hex characters")]
pub struct NonceError;

impl Nonce {
    /// A nonce from its raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parses 64 lowercase hex characters; uppercase or any other length is refused.
    pub fn parse(text: &str) -> Result<Self, NonceError> {
        if text.len() != 64 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(NonceError);
        }
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(text, &mut bytes).map_err(|_| NonceError)?;
        Ok(Self(bytes))
    }

    /// The 64-character lowercase hex spelling.
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl Serialize for Nonce {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Nonce {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = std::borrow::Cow::<str>::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// What the agent may ask of the helper. The set is closed: there is no write variant and no
/// path, command, unit name or argument vector, so none can be smuggled through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperRequest {
    /// Is the helper alive, and which build.
    Health,
    /// Which groups are enabled and what the platform allows.
    Groups,
    /// Whether a Docker or Podman socket answers, and which.
    DockerProbe,
    /// Every container, running or not.
    DockerList,
    /// Container events in a window given as seconds before now: from `since_secs` ago to
    /// `until_secs` ago, with `since_secs` greater and at most 86 400.
    DockerEvents {
        /// Start of the window, in seconds before now.
        since_secs: u32,
        /// End of the window, in seconds before now.
        until_secs: u32,
    },
    /// The inspect document of one container, with environment values removed.
    DockerInspect {
        /// The container.
        container: ContainerRef,
    },
    /// The last lines of one container's output.
    DockerLogs {
        /// The container.
        container: ContainerRef,
        /// How many lines, 1 to 500.
        lines: u16,
    },
    /// The SMART-capable devices the allowlist admits.
    SmartScan,
    /// The SMART report of one allowed device.
    SmartReport {
        /// The device.
        device: DeviceRef,
    },
    /// Recent kernel ring buffer lines.
    KernelLog {
        /// How far back, 1 to 1 440 minutes.
        since_minutes: u32,
        /// How many lines, 1 to 500.
        lines: u16,
    },
    /// Listening sockets with their owning processes.
    NetListeners,
    /// A safety attestation: the helper signs a report of what it enforces, bound to `nonce`.
    SafetyAttest {
        /// The challenge to bind into the attestation.
        nonce: Nonce,
    },
    /// The helper's public attestation key. Read-only: it carries no secret and changes nothing.
    SafetyPublicKey,
    /// The loaded configuration and files of one systemd service, as one identity hash.
    UnitIdentity {
        /// The unit as typed.
        name: UnitName,
    },
    /// Restarts one service, if it still has the identity that was approved.
    UnitRestart {
        /// The canonical id the manager reports for the unit.
        id: UnitId,
        /// The identity hash that was approved.
        identity_hash: IdentityHash,
    },
    /// Step one of a block: the element goes in with the 30 second kernel timeout.
    FirewallApply {
        /// The block id, the execution id of this apply.
        block: Uuid,
        /// The target as the agent canonicalised it; the helper parses it again.
        source: BlockSource,
        /// The TTL the block has after `confirm`, in seconds.
        ttl_s: u32,
        /// What the approval bound; absent is refused as `malformed_safety`.
        #[serde(default)]
        expected_safety: Option<ExpectedSafety>,
    },
    /// Step two of a block: the element gets its full TTL.
    FirewallConfirm {
        /// The block id.
        block: Uuid,
        /// The target.
        source: BlockSource,
        /// The TTL, equal to the apply's.
        ttl_s: u32,
        /// What the approval bound; absent is refused as `malformed_safety`.
        #[serde(default)]
        expected_safety: Option<ExpectedSafety>,
    },
    /// Removes a block. Never refused for a safety reason, so it carries no safety state.
    FirewallRollback {
        /// The block id.
        block: Uuid,
        /// The target.
        source: BlockSource,
        /// The TTL of the block, for the journal match.
        ttl_s: u32,
    },
    /// The blocks the helper holds and the state of the gate, the drills and the table.
    FirewallListBlocks,
}

/// One request on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperRequestFrame {
    /// The helper protocol number, [`super::HELPER_PROTOCOL`].
    pub v: u16,
    /// Correlates the answer with this request.
    pub id: Uuid,
    /// The request.
    pub request: HelperRequest,
}

impl HelperRequest {
    /// The group an operation belongs to, or `None` for `Health` and `Groups`.
    #[must_use]
    pub const fn group(&self) -> Option<HelperGroup> {
        match self {
            Self::Health | Self::Groups => None,
            Self::DockerProbe
            | Self::DockerList
            | Self::DockerEvents { .. }
            | Self::DockerInspect { .. }
            | Self::DockerLogs { .. } => Some(HelperGroup::DockerRead),
            Self::SmartScan | Self::SmartReport { .. } => Some(HelperGroup::SmartRead),
            Self::KernelLog { .. } => Some(HelperGroup::KernelRead),
            Self::NetListeners => Some(HelperGroup::NetRead),
            Self::SafetyAttest { .. } | Self::SafetyPublicKey => Some(HelperGroup::SafetyAttest),
            Self::UnitIdentity { .. } => Some(HelperGroup::SystemdRead),
            Self::UnitRestart { .. } => Some(HelperGroup::SystemdWrite),
            Self::FirewallApply { .. }
            | Self::FirewallConfirm { .. }
            | Self::FirewallRollback { .. }
            | Self::FirewallListBlocks => Some(HelperGroup::Nft),
        }
    }
}
