//! Baselines: what a host is expected to run.

use jiff::Timestamp;

use crate::id::HostId;

/// What a baseline entry describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineKind {
    /// A container.
    Container,
    /// A systemd unit.
    Unit,
    /// A listening port.
    Port,
}

/// How bad it is when an expected entry is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Criticality {
    /// Missing is critical.
    Critical,
    /// Missing is a warning.
    Warning,
}

/// One expected container, unit or port on a host.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BaselineEntry {
    /// The host.
    pub host: HostId,
    /// What it describes.
    pub kind: BaselineKind,
    /// Container name, unit name or `proto/port`.
    pub key: String,
    /// How bad it is when missing.
    pub criticality: Criticality,
    /// When it was learned.
    pub learned_at: Timestamp,
    /// When it was last seen; entries unseen for long enough expire.
    pub last_seen_at: Timestamp,
    /// Whether it came from a managed source (compose or Portainer labels).
    pub managed: bool,
}
