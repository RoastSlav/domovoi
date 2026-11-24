//! The helper's operation groups.

use serde::{Deserialize, Serialize};

/// A named set of operations the helper may serve, as written in `helper.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HelperGroup {
    /// Read-only Docker queries.
    #[serde(rename = "docker.read")]
    DockerRead,
    /// Docker changes.
    #[serde(rename = "docker.write")]
    DockerWrite,
    /// Read-only systemd queries.
    #[serde(rename = "systemd.read")]
    SystemdRead,
    /// systemd changes.
    #[serde(rename = "systemd.write")]
    SystemdWrite,
    /// Firewall changes.
    #[serde(rename = "nft")]
    Nft,
    /// SMART disk reads.
    #[serde(rename = "smart.read")]
    SmartRead,
    /// Kernel log reads.
    #[serde(rename = "kernel.read")]
    KernelRead,
    /// Network state reads.
    #[serde(rename = "net.read")]
    NetRead,
    /// Signing a safety attestation.
    #[serde(rename = "safety.attest")]
    SafetyAttest,
}

impl HelperGroup {
    /// Every group.
    pub const ALL: &'static [Self] = &[
        Self::DockerRead,
        Self::DockerWrite,
        Self::SystemdRead,
        Self::SystemdWrite,
        Self::Nft,
        Self::SmartRead,
        Self::KernelRead,
        Self::NetRead,
        Self::SafetyAttest,
    ];

    /// The name used in `helper.toml` and on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DockerRead => "docker.read",
            Self::DockerWrite => "docker.write",
            Self::SystemdRead => "systemd.read",
            Self::SystemdWrite => "systemd.write",
            Self::Nft => "nft",
            Self::SmartRead => "smart.read",
            Self::KernelRead => "kernel.read",
            Self::NetRead => "net.read",
            Self::SafetyAttest => "safety.attest",
        }
    }

    /// Whether the group changes the host. Exactly `docker.write`, `systemd.write` and `nft`.
    #[must_use]
    pub const fn is_write(self) -> bool {
        matches!(self, Self::DockerWrite | Self::SystemdWrite | Self::Nft)
    }

    /// The group named `text`, or `None` for an unknown name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|g| g.as_str() == text)
    }
}

impl std::fmt::Display for HelperGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The read groups a `Read` request may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReadGroup {
    /// Read-only Docker queries.
    #[serde(rename = "docker.read")]
    DockerRead,
    /// Read-only systemd queries.
    #[serde(rename = "systemd.read")]
    SystemdRead,
    /// SMART disk reads.
    #[serde(rename = "smart.read")]
    SmartRead,
    /// Kernel log reads.
    #[serde(rename = "kernel.read")]
    KernelRead,
    /// Network state reads.
    #[serde(rename = "net.read")]
    NetRead,
}

impl ReadGroup {
    /// The matching [`HelperGroup`].
    #[must_use]
    pub const fn group(self) -> HelperGroup {
        match self {
            Self::DockerRead => HelperGroup::DockerRead,
            Self::SystemdRead => HelperGroup::SystemdRead,
            Self::SmartRead => HelperGroup::SmartRead,
            Self::KernelRead => HelperGroup::KernelRead,
            Self::NetRead => HelperGroup::NetRead,
        }
    }
}
