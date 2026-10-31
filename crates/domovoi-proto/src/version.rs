//! Protocol versions and which hub and agent versions can talk.

use std::fmt;

/// A protocol version. The major number is also the envelope's `v` field.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ProtocolVersion {
    /// Incremented for breaking changes; peers with different majors never talk.
    pub major: u16,
    /// Incremented for additive changes.
    pub minor: u16,
}

/// The version this build speaks.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

/// Whether a hub at `hub` accepts an agent at `agent`: the same major, and the agent
/// at most one minor behind and never ahead.
#[must_use]
pub fn compatible(agent: ProtocolVersion, hub: ProtocolVersion) -> bool {
    agent.major == hub.major
        && agent.minor <= hub.minor
        && agent.minor.saturating_add(1) >= hub.minor
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}
