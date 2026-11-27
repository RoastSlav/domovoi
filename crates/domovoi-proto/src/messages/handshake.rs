//! Enrollment and the connection handshake.

use std::collections::BTreeMap;
use std::fmt;

use domovoi_core::{AgentId, CheckKey, HostId, HostName, SkillSummary};
use jiff::Timestamp;
use serde_json::Value;
use uuid::Uuid;

use crate::bytes::{CertFingerprint, EnrollToken, Nonce, PublicKey, Sha256Bytes, Signature};
use crate::version::{ProtocolVersion, Release};

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

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
    /// Whether the container runtime may be used on this host: `Some(true)` or
    /// `Some(false)` is the hub's choice, `None` leaves it to the agent. An agent whose
    /// `agent.toml` sets `[docker] enabled` to `true` or `false` ignores it (ADR 0018).
    #[serde(default)]
    pub docker: Option<bool>,
}

/// What an agent found out about the container runtime on its host.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DockerReport {
    /// Which engine answers on the endpoint, such as `Docker` or `Podman`. Empty when none
    /// was found.
    #[serde(default)]
    pub engine: String,
    /// The endpoint in use, or the first one tried. Empty when none was configured or found.
    #[serde(default)]
    pub endpoint: String,
    /// Whether an engine answered on the endpoint at startup, so the hub may switch the
    /// integration on.
    pub available: bool,
    /// Whether `agent.toml` fixes the setting (`enabled = true` or `false`), so the hub
    /// cannot change it.
    pub locked: bool,
    /// Whether the integration was in use when the agent connected, before any hub choice.
    pub enabled: bool,
    /// Why it is unavailable, in a sentence, when it is.
    #[serde(default)]
    pub problem: Option<String>,
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

/// A protocol feature an agent sends in `hello.features`: every action execution it can
/// perform is recorded in its journal before it runs, so a journal with no record of an
/// execution is proof that the execution never arrived.
///
/// Without this, absence is not proof: an older agent, or one that still serves an
/// unjournaled execution path, can execute a run and answer `not_found`. The hub must then
/// treat the answer as an unknowable outcome instead of "nothing was started".
pub const AUTHORITATIVE_ABSENCE: &str = "authoritative_absence";

/// The first frame of every connection by an enrolled agent.
///
/// The hub replies `challenge`, or `reject` if the agent is unknown, revoked, speaks an
/// incompatible protocol or runs an older release than this hub accepts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hello {
    /// The identity from `enrolled`.
    pub agent_id: AgentId,
    /// The protocol version the agent speaks.
    pub protocol: ProtocolVersion,
    /// The agent's version.
    pub agent_version: String,
    /// The build the agent runs, such as a git commit. Two builds of one release differ
    /// here, so the hub can tell a swapped binary. Absent from older agents and from builds
    /// that do not know their commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_build: Option<String>,
    /// Facts about the host. Boxed to keep the shared `Message` enum small; the wire shape is
    /// unchanged.
    pub facts: Box<HostFacts>,
    /// Optional features, such as `docker` or `journal`.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Protocol features the agent implements, sorted. Empty while the protocol defines no optional feature.
    #[serde(default)]
    pub features: Vec<String>,
    /// The action journal's epoch, so the hub can tell a lost journal from a new one.
    #[serde(default = "default_epoch")]
    pub journal_epoch: Uuid,
    /// Hash of the capability catalogue. `None` until the catalogue exists.
    #[serde(default)]
    pub catalog_hash: Option<Sha256Bytes>,
    /// Hash of the unattended ceiling. `None` until it exists.
    #[serde(default)]
    pub ceiling_hash: Option<Sha256Bytes>,
    /// A short description of the unattended ceiling. `None` until it exists.
    #[serde(default)]
    pub ceiling_summary: Option<String>,
    /// What the skill runner reports about itself. `absent` until it exists.
    #[serde(default)]
    pub skill_runner_status: SkillRunnerStatus,
    /// What the helper reports about itself. `absent` until it exists.
    #[serde(default)]
    pub helper_status: HelperStatus,
    /// Whether the agent's effective uid is 0.
    #[serde(default)]
    pub runs_as_root: bool,
    /// Whether the agent found that it runs inside a container. The hub labels the host and
    /// says why host-level actions are not offered. `false` from older agents.
    #[serde(default)]
    pub in_container: bool,
    /// The helper's public key when a helper is present. `None` until it exists.
    #[serde(default)]
    pub helper_identity: Option<HelperIdentity>,
    /// The skills the agent has, with manifest and entry-script hashes.
    #[serde(default)]
    pub skills: Vec<SkillSummary>,
    /// Every check the agent knows, with its local settings before any hub override, so
    /// the hub can show what an override changes. Empty from older agents.
    #[serde(default)]
    pub checks: Vec<CheckInfo>,
    /// What the agent found out about the container runtime. `None` from older agents.
    #[serde(default)]
    pub docker: Option<DockerReport>,
    /// Every skill of a package stored on the host, enabled or not, so the hub can list the
    /// ones that are off with the reason. Empty from older agents and from hosts with no packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub package_skills: Vec<PackageSkillState>,
}

/// One skill of a stored package and whether it can run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PackageSkillState {
    /// The package the skill came from.
    pub package: String,
    /// The stored version.
    pub version: String,
    /// The skill's name.
    pub skill: String,
    /// Whether the skill is in the agent's catalogue and can run.
    pub enabled: bool,
    /// Why it is not enabled; absent when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn default_epoch() -> Uuid {
    Uuid::nil()
}

/// What the agent knows about the skill runner.
///
/// A status says what is true now, not what the product intends. Only the agent produces
/// these, so the enum is closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SkillRunnerStatus {
    /// No skill runner is installed or configured.
    #[default]
    Absent,
    /// A runner exists but this host cannot use it.
    Unsupported {
        /// Why it cannot be used.
        reason: String,
    },
    /// A runner exists but reported an error.
    Error {
        /// What went wrong.
        reason: String,
    },
    /// The runner is present and working.
    Ok {
        /// The runner's version.
        version: String,
    },
}

/// What the agent knows about the privileged helper.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HelperStatus {
    /// No helper is installed or configured.
    #[default]
    Absent,
    /// The helper is present and offers `ops`.
    Ok {
        /// The operations the helper offers.
        ops: Vec<String>,
    },
    /// The helper exists but reported an error.
    Error {
        /// What went wrong.
        reason: String,
    },
}

/// The helper's public key, as the agent reports it.
///
/// The hub computes the fingerprint itself (SHA-256 of the 32 key bytes) and pins only
/// through owner enrollment. The agent never sends a fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HelperIdentity {
    /// Base64 (standard, padded) of exactly 32 raw Ed25519 public-key bytes.
    pub public_key: PublicKey,
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
    /// The protocol versions are not equal.
    Incompatible {
        /// The hub's version.
        hub: ProtocolVersion,
    },
    /// No longer sent by the hub; kept so frames from older hubs still decode.
    AgentTooOld {
        /// The oldest agent release this hub accepts.
        minimum: Release,
    },
    /// The challenge signature did not verify.
    BadSignature,
    /// The enrollment token is unknown, used or expired.
    BadToken,
    /// A reason this build does not know.
    #[serde(other)]
    Unknown,
}
