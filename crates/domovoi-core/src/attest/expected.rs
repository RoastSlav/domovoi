//! The safety state a containment request expects, as it travels on the wire.

use serde::{Deserialize, Deserializer, Serialize};

use super::boot::BootId;
use super::hash::Hash32;
use super::identity::SafetyIdentity;
use super::reason::UnverifiedReason;

/// A request's expectation of the helper's safety state.
///
/// Authority-bearing, so it is strict in both directions: an extra field, an unknown tag, an
/// unknown reason, a malformed hash or boot id and a `null` for a field are all refused rather
/// than preserved. An `Unverified` value serialises to exactly `state` and `reason`; it carries
/// no identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExpectedSafety {
    /// The caller verified the helper and expects exactly this identity.
    Verified {
        /// The fingerprint of the pinned helper key, 64 lowercase hex characters.
        fingerprint: Hash32,
        /// The boot the attestation was made in.
        boot_id: BootId,
        /// The helper's generation.
        generation: u64,
        /// The hash of the effective configuration.
        config_hash: Hash32,
        /// The hash of the observed route state.
        route_state_hash: Hash32,
    },
    /// The caller could not verify the helper, and says why.
    Unverified {
        /// Why the helper is unverified.
        reason: UnverifiedReason,
    },
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Tag {
    Verified,
    Unverified,
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// Every field of either variant, optional, so unknown fields are refused by serde and the
/// per-tag field sets are checked by hand.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    state: Tag,
    #[serde(default, deserialize_with = "present")]
    fingerprint: Option<Hash32>,
    #[serde(default, deserialize_with = "present")]
    boot_id: Option<BootId>,
    #[serde(default, deserialize_with = "present")]
    generation: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    config_hash: Option<Hash32>,
    #[serde(default, deserialize_with = "present")]
    route_state_hash: Option<Hash32>,
    #[serde(default, deserialize_with = "present")]
    reason: Option<UnverifiedReason>,
}

impl<'de> Deserialize<'de> for ExpectedSafety {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let raw = Raw::deserialize(deserializer)?;
        match (raw.state, raw) {
            (
                Tag::Verified,
                Raw {
                    fingerprint: Some(fingerprint),
                    boot_id: Some(boot_id),
                    generation: Some(generation),
                    config_hash: Some(config_hash),
                    route_state_hash: Some(route_state_hash),
                    reason: None,
                    ..
                },
            ) => Ok(Self::Verified {
                fingerprint,
                boot_id,
                generation,
                config_hash,
                route_state_hash,
            }),
            (
                Tag::Unverified,
                Raw {
                    fingerprint: None,
                    boot_id: None,
                    generation: None,
                    config_hash: None,
                    route_state_hash: None,
                    reason: Some(reason),
                    ..
                },
            ) => Ok(Self::Unverified { reason }),
            _ => Err(D::Error::custom(
                "the field set differs from the one the state tag defines",
            )),
        }
    }
}

impl From<&SafetyIdentity> for ExpectedSafety {
    fn from(identity: &SafetyIdentity) -> Self {
        Self::Verified {
            fingerprint: identity.fingerprint.into(),
            boot_id: identity.boot_id.clone(),
            generation: identity.generation,
            config_hash: identity.config_hash,
            route_state_hash: identity.route_state_hash,
        }
    }
}

impl ExpectedSafety {
    /// The identity a `Verified` value names; `None` for `Unverified`.
    #[must_use]
    pub fn identity(&self) -> Option<SafetyIdentity> {
        match self {
            Self::Verified {
                fingerprint,
                boot_id,
                generation,
                config_hash,
                route_state_hash,
            } => Some(SafetyIdentity {
                fingerprint: (*fingerprint).into(),
                boot_id: boot_id.clone(),
                generation: *generation,
                config_hash: *config_hash,
                route_state_hash: *route_state_hash,
            }),
            Self::Unverified { .. } => None,
        }
    }

    /// Parses JSON text strictly.
    ///
    /// # Errors
    ///
    /// [`MalformedSafety::Invalid`] for any deviation from the two shapes.
    pub fn parse_json(text: &str) -> Result<Self, MalformedSafety> {
        serde_json::from_str(text).map_err(|err| MalformedSafety::Invalid(err.to_string()))
    }
}

/// Why a request's `expected_safety` is refused as `malformed_safety`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MalformedSafety {
    /// A containment request carried no `expected_safety`. Absence is never read as "no
    /// guard".
    #[error("expected_safety is required for containment")]
    Missing,
    /// The value is not one of the two strict shapes.
    #[error("expected_safety is malformed: {0}")]
    Invalid(String),
}

/// The one function that enforces "`expected_safety` is required for containment", used by the
/// agent and by the helper's request decoder.
///
/// # Errors
///
/// [`MalformedSafety::Missing`] when the request carried no tag.
pub fn require_expected_safety(
    tag: Option<&ExpectedSafety>,
) -> Result<&ExpectedSafety, MalformedSafety> {
    tag.ok_or(MalformedSafety::Missing)
}
