//! Target identity (RW-34): what a plan binds, as opposed to what an owner typed.
//!
//! A policy rule names a target (`plex`); a permit and a plan hash bind a canonical identity.
//! A host identity is its `HostId` plus the fingerprint of its enrolled agent key, written by
//! the one formatter the action layer uses too, so a re-enrolment invalidates a bound plan.
//! A container identity embeds that host text before the full 64-character Docker id, so a
//! rename keeps identity and a recreate does not. Nothing here guesses an identity: one that
//! cannot be proven is refused, never replaced by a name or a short id.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::availability::Availability;
use super::effect::TargetKind;
use super::ids::{TargetIdentity, TargetName};
use crate::containment::Net;
use crate::error::ValidationError;
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::HostId;

/// A Docker container's full id: exactly 64 ASCII lowercase hex characters.
///
/// A short id, such as Docker's 12-character display form, is refused rather than expanded:
/// RW-34 binds the full id, and an expanded one could stop matching the same container.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContainerId(String);

impl ContainerId {
    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validates `input` as a full container id.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.len() != 64 {
            return Err(ValidationError::new(
                "container id",
                "must be exactly 64 characters, not a short id",
            ));
        }
        if !input
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(ValidationError::new(
                "container id",
                "must be lowercase hex",
            ));
        }
        Ok(Self(input.to_owned()))
    }
}

impl fmt::Display for ContainerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ContainerId {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<ContainerId> for String {
    fn from(value: ContainerId) -> Self {
        value.0
    }
}

/// What a caller wants identified: a host, a container by name, or a unit by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetQuery {
    /// The host itself.
    Host {
        /// The host to identify.
        host: HostId,
    },
    /// One container on a host, by the name an owner typed or the agent reported.
    Container {
        /// The host whose container runtime to read.
        host: HostId,
        /// The container name.
        name: TargetName,
    },
    /// One systemd unit on a host.
    Unit {
        /// The host whose unit to identify.
        host: HostId,
        /// The unit name.
        name: TargetName,
    },
    /// One external network address or prefix, blocked on a host.
    NetworkSource {
        /// The host that would hold the block.
        host: HostId,
        /// The canonical network.
        source: Net,
    },
}

impl TargetQuery {
    /// The host the query asks about.
    #[must_use]
    pub fn host(&self) -> HostId {
        match self {
            Self::Host { host }
            | Self::Container { host, .. }
            | Self::Unit { host, .. }
            | Self::NetworkSource { host, .. } => *host,
        }
    }

    /// The kind this query names a target of.
    #[must_use]
    pub fn kind(&self) -> TargetKind {
        match self {
            Self::Host { .. } => TargetKind::Host,
            Self::Container { .. } => TargetKind::Container,
            Self::Unit { .. } => TargetKind::Unit,
            Self::NetworkSource { .. } => TargetKind::NetworkSource,
        }
    }
}

/// The identity a plan binds, plus what a caller needs to show and explain it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTarget {
    /// The complete canonical identity, the enrolled key fingerprint included.
    pub identity: TargetIdentity,
    /// The name the owner typed or the agent reported: display only, never bound.
    pub name: TargetName,
    /// What kind of target this is.
    pub kind: TargetKind,
    /// The enrolled key fingerprint inside `identity`, computed here from the hub's own
    /// enrolment record.
    pub host_key: KeyFingerprint,
}

/// Why a target could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    /// The target does not resolve now: it is gone, its identity is not provable, or the
    /// host has no usable enrolment. Never optimistic.
    #[error("the target does not resolve now: {reason}")]
    Unresolved {
        /// The query that failed.
        query: TargetQuery,
        /// Why.
        reason: TargetUnresolved,
    },
    /// More than one container has this name, so none is chosen.
    #[error("this name belongs to {count} containers, so none is bound")]
    Ambiguous {
        /// The query that matched more than once.
        query: TargetQuery,
        /// How many matched.
        count: usize,
    },
    /// The name is not valid for its kind, checked before any host is read.
    #[error("the target name is not valid: {reason}")]
    InvalidName {
        /// The query with the bad name.
        query: TargetQuery,
        /// What is wrong with the name.
        reason: ValidationError,
    },
    /// The helper refused the unit; the refusal says why, in the words of the token table.
    #[error("the unit was refused: {}", refusal.describe())]
    UnitRefused {
        /// The query the helper refused.
        query: TargetQuery,
        /// The helper's refusal.
        refusal: crate::unit::UnitRefusal,
    },
    /// The host cannot answer: no session, another protocol, or no container runtime.
    #[error("the host cannot answer this target")]
    HostUnavailable {
        /// The host that cannot answer.
        host: HostId,
        /// Its availability, which says why.
        state: Availability,
    },
}

/// Why a target's identity is not provable now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum TargetUnresolved {
    /// No container has this name now.
    #[error("no container has this name now")]
    ContainerVanished,
    /// The inspect document carried no full id, so identity is not provable and nothing is
    /// bound.
    #[error("the container's full id is not provable")]
    ContainerIdentityUnproven,
    /// The host has no usable enrolled agent key, so it has no canonical identity.
    #[error("the host has no usable enrolled agent key")]
    HostIdentityUnproven,
    /// **R-8 is open:** a systemd unit's effective-configuration identity (its fragment plus
    /// every drop-in) is not proven by this build, so a unit identity is never claimed.
    #[error("a unit's effective-configuration identity is not proven by this build")]
    UnitIdentityUnproven,
}

impl TargetUnresolved {
    /// A short owner-facing sentence, in particular that an unproven identity is never
    /// replaced by a guess.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Self::ContainerVanished => "no container has this name now",
            Self::ContainerIdentityUnproven => {
                "this container's full id cannot be read, so nothing is bound"
            }
            Self::HostIdentityUnproven => {
                "this host has no usable enrolled agent key, so it has no identity"
            }
            Self::UnitIdentityUnproven => {
                "a unit's effective configuration is not read by this build, so its identity is unproven"
            }
        }
    }
}

/// RW-34's container-name grammar, anchored: `[a-zA-Z0-9][a-zA-Z0-9_.-]+`, a full match.
///
/// Run before any resolution: a name that fails here never reaches an agent. As written in
/// RW-34 the grammar needs at least two characters, so a single-character name is refused.
pub fn validate_container_name(text: &str) -> Result<TargetName, ValidationError> {
    let chars = text.chars().collect::<Vec<char>>();
    if chars.len() < 2 {
        return Err(ValidationError::new(
            "container name",
            "must be at least two characters",
        ));
    }
    if !chars[0].is_ascii_alphanumeric() {
        return Err(ValidationError::new(
            "container name",
            "must start with a letter or a digit",
        ));
    }
    let rest = &chars[1..];
    if !rest
        .iter()
        .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-'))
    {
        return Err(ValidationError::new(
            "container name",
            "may contain only letters, digits, dot, underscore and hyphen",
        ));
    }
    TargetName::parse(text)
}

/// The canonical host identity: `host:{host_id}/{fingerprint}`.
///
/// Parses the one formatter the action layer also uses, so a plan hash and a registry
/// resolution cannot disagree about the format. The fingerprint is computed from the hub's
/// enrolled record, never accepted from a caller.
pub fn canonical_host_identity(
    host: HostId,
    key: &KeyFingerprint,
) -> Result<TargetIdentity, ValidationError> {
    TargetIdentity::parse(&host_target_text(host, key))
}

/// The canonical container identity: the host identity, then the full container id.
///
/// Re-enrolling the host changes the embedded host text, so a container-targeted approval is
/// invalidated the same way a host-targeted one is.
pub fn canonical_container_identity(
    host: HostId,
    key: &KeyFingerprint,
    id: &ContainerId,
) -> Result<TargetIdentity, ValidationError> {
    let text = format!("container:{host}/{key}/{id}");
    TargetIdentity::parse(&text)
}

/// The canonical network identity: `ipnet:{canonical}@host:{host_id}/{fingerprint}`.
///
/// The only place the format is written. It embeds the host text, so a re-enrolment
/// invalidates a bound block approval the way it does for a host.
pub fn canonical_network_identity(
    host: HostId,
    key: &KeyFingerprint,
    net: &Net,
) -> Result<TargetIdentity, ValidationError> {
    TargetIdentity::parse(&format!(
        "ipnet:{}@{}",
        net.canonical(),
        host_target_text(host, key)
    ))
}
