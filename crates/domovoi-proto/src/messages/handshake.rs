//! Enrollment and the connection handshake.

use std::collections::BTreeMap;
use std::fmt;

use domovoi_core::{AgentId, CheckKey, HostId, HostName, SkillSummary};
use jiff::Timestamp;
use serde_json::Value;
use uuid::Uuid;

use crate::bytes::{CertFingerprint, EnrollToken, Nonce, PublicKey, Signature};
use crate::version::ProtocolVersion;

/// Identifies one authenticated connection, from `welcome` until it closes.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct SessionId(Uuid);

impl SessionId {
    /// A fresh time-ordered id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Wraps an existing UUID.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SessionId({})", self.0)
    }
}

/// What an agent reports about its host.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostFacts {
    /// The kernel's hostname.
    pub hostname: String,
    /// Operating system name and version, such as `Debian GNU/Linux 13`.
    pub os: String,
    /// Kernel release.
    pub kernel: String,
    /// CPU architecture, such as `x86_64`.
    pub arch: String,
    /// Logical CPUs.
    pub cpu_count: u32,
    /// Total memory in bytes.
    pub mem_total_bytes: u64,
    /// When the host booted.
    pub boot_time: Timestamp,
    /// The kernel's boot id; a new value means the host rebooted.
    pub boot_id: String,
}

/// Settings the hub manages for an agent.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct AgentSettings {
    /// How often the agent pings when idle.
    pub heartbeat_seconds: u32,
    /// Per-check changes on top of the agent's local config.
    #[serde(default)]
    pub check_overrides: BTreeMap<CheckKey, CheckOverride>,
}

/// Changes to one check. Absent fields keep the agent's local value.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct CheckOverride {
    /// Turns the check on or off.
    pub enabled: Option<bool>,
    /// How often it runs, in seconds.
    pub interval_seconds: Option<u32>,
    /// Check-specific thresholds, merged over the local ones.
    #[serde(default)]
    pub thresholds: BTreeMap<String, Value>,
}

/// Sent by a new agent, once, to exchange an enrollment token for an identity.
///
/// The agent generates its ed25519 key pair first and sends the public half. The hub
/// replies `enrolled` or `reject` (`bad_token`), then closes the connection.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Enroll {
    /// The single-use token from `domovoi hub token create`.
    pub token: EnrollToken,
    /// The agent's new public key.
    pub public_key: PublicKey,
    /// The host name the agent wants.
    pub host_name: HostName,
    /// Facts about the host.
    pub facts: HostFacts,
    /// The agent's version.
    pub agent_version: String,
}

/// The hub's reply to a valid `enroll`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Enrolled {
    /// The agent's identity, used in every `hello`.
    pub agent_id: AgentId,
    /// The host the agent is bound to.
    pub host_id: HostId,
    /// The hub certificate's fingerprint. The agent checks it matches the certificate it
    /// saw on this connection and pins it for all later connections.
    pub hub_cert_sha256: CertFingerprint,
}

/// The first frame of every connection by an enrolled agent.
///
/// The hub replies `challenge`, or `reject` if the agent is unknown, revoked or
/// speaks an incompatible protocol.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hello {
    /// The identity from `enrolled`.
    pub agent_id: AgentId,
    /// The protocol version the agent speaks.
    pub protocol: ProtocolVersion,
    /// The agent's version.
    pub agent_version: String,
    /// Facts about the host.
    pub facts: HostFacts,
    /// Optional features, such as `docker` or `journal`.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The skills the agent has, with manifest hashes.
    #[serde(default)]
    pub skills: Vec<SkillSummary>,
    /// Every check the agent knows, with its local settings before any hub override, so
    /// the hub can show what an override changes. Empty from older agents.
    #[serde(default)]
    pub checks: Vec<CheckInfo>,
}

/// One check as the agent's own config sets it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CheckInfo {
    /// The check.
    pub key: CheckKey,
    /// Whether `agent.toml` leaves it on.
    pub enabled: bool,
    /// How often it runs, in seconds.
    pub interval_seconds: u32,
    /// Thresholds from `agent.toml`; the check's built-in defaults apply to the rest.
    #[serde(default)]
    pub thresholds: BTreeMap<String, Value>,
}

/// The hub's random challenge after `hello`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Challenge {
    /// 32 random bytes.
    pub nonce: Nonce,
}

/// The agent's signature over [`crate::challenge_message`] for the received nonce.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChallengeResponse {
    /// The ed25519 signature.
    pub signature: Signature,
}

/// The hub accepted the signature; the session is open.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Welcome {
    /// This connection's id.
    pub session_id: SessionId,
    /// The hub's version.
    pub hub_version: String,
    /// Settings the agent applies now.
    pub config: AgentSettings,
}

/// The hub refused the connection and will close it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Reject {
    /// Why.
    pub reason: RejectReason,
}

/// Why the hub refused a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum RejectReason {
    /// No agent has this id.
    UnknownAgent,
    /// The agent was revoked.
    Revoked,
    /// The protocol versions are not compatible.
    Incompatible {
        /// The hub's version.
        hub: ProtocolVersion,
    },
    /// The challenge signature did not verify.
    BadSignature,
    /// The enrollment token is unknown, used or expired.
    BadToken,
    /// A reason this build does not know.
    #[serde(other)]
    Unknown,
}
