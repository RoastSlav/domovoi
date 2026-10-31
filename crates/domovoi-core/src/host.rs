//! Hosts: the machines Domovoi watches.

use std::collections::BTreeMap;
use std::fmt;

use jiff::Timestamp;

use crate::error::ValidationError;
use crate::id::{AgentId, HostId};

/// A validated host slug matching `^[a-z0-9][a-z0-9-]{0,62}$`.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct HostName(String);

impl HostName {
    /// Validates `input` as a host slug.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let bytes = input.as_bytes();
        if bytes.is_empty() {
            return Err(ValidationError::new("host name", "must not be empty"));
        }
        if bytes.len() > 63 {
            return Err(ValidationError::new(
                "host name",
                "must be at most 63 characters",
            ));
        }
        let first_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
        if !first_ok {
            return Err(ValidationError::new(
                "host name",
                "must start with a lowercase letter or digit",
            ));
        }
        let rest_ok = bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-');
        if !rest_ok {
            return Err(ValidationError::new(
                "host name",
                "may contain only lowercase letters, digits and '-'",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The slug as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for HostName {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<HostName> for String {
    fn from(value: HostName) -> Self {
        value.0
    }
}

/// How Domovoi learns about a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HostKind {
    /// The machine the hub runs on.
    Local,
    /// A machine with an enrolled agent.
    Remote,
    /// Known only from webhooks; has no agent.
    External,
}

/// A machine Domovoi watches.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Host {
    /// Stable identifier.
    pub id: HostId,
    /// Unique slug shown to humans.
    pub name: HostName,
    /// How the host is observed.
    pub kind: HostKind,
    /// Free-form labels for grouping and matching.
    pub labels: BTreeMap<String, String>,
    /// When the host was first registered.
    pub created_at: Timestamp,
}

/// The enrolled agent of a host (ARCHITECTURE.md §5.1).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AgentIdentity {
    /// Stable identifier of the agent.
    pub agent: AgentId,
    /// The host the agent runs on.
    pub host: HostId,
    /// The agent's ed25519 public key.
    #[serde(with = "crate::sha256::hex32")]
    pub public_key: [u8; 32],
    /// Agent version reported at enrollment or on the last handshake.
    pub version: String,
    /// When the agent enrolled.
    pub enrolled_at: Timestamp,
    /// When the agent was revoked; a revoked agent may never connect again.
    pub revoked_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_name_accepts_valid_and_rejects_invalid() {
        let long_ok = "a".repeat(63);
        let too_long = "a".repeat(64);
        let valid = ["nas", "media-1", "0day", "a", long_ok.as_str()];
        for name in valid {
            assert!(HostName::parse(name).is_ok(), "{name} should be valid");
        }
        let invalid = [
            "",
            "NAS",
            "-nas",
            "nas_1",
            "nas.local",
            "nás",
            too_long.as_str(),
        ];
        for name in invalid {
            assert!(HostName::parse(name).is_err(), "{name:?} should be invalid");
        }
    }

    #[test]
    fn host_name_deserialization_validates() {
        assert!(serde_json::from_str::<HostName>("\"Bad Name\"").is_err());
        assert!(serde_json::from_str::<HostName>("\"good\"").is_ok());
    }
}
