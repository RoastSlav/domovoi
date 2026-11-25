//! The safety identity: which helper key, boot, generation and effective sets a decision rests
//! on.

use crate::host::KeyFingerprint;

use super::boot::BootId;
use super::hash::Hash32;
use super::scalar::parse_uint;

impl From<Hash32> for KeyFingerprint {
    fn from(hash: Hash32) -> Self {
        Self::from_raw(*hash.as_bytes())
    }
}

impl From<KeyFingerprint> for Hash32 {
    fn from(fingerprint: KeyFingerprint) -> Self {
        Self::from_bytes(*fingerprint.as_bytes())
    }
}

/// Why text is not a canonical [`SafetyIdentity`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a canonical safety identity: {0}")]
pub struct IdentityError(&'static str);

/// What a helper attestation proved at one moment: the pinned key's fingerprint, the boot, the
/// generation and the hashes of the effective configuration and route state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafetyIdentity {
    /// The fingerprint of the pinned helper key, computed by the hub.
    pub fingerprint: KeyFingerprint,
    /// The boot the report was made in.
    pub boot_id: BootId,
    /// The generation, at least 1.
    pub generation: u64,
    /// The hash of the effective configuration.
    pub config_hash: Hash32,
    /// The hash of the observed route state.
    pub route_state_hash: Hash32,
}

impl SafetyIdentity {
    /// The canonical text, stored in `Authorization.safety_identity` and, as evidence, in a
    /// path confirmation:
    /// `v1:<fingerprint>:<boot_id>:<generation>:<config_hash>:<route_state_hash>`.
    #[must_use]
    pub fn canonical(&self) -> String {
        format!(
            "v1:{}:{}:{}:{}:{}",
            self.fingerprint,
            self.boot_id,
            self.generation,
            self.config_hash,
            self.route_state_hash
        )
    }

    /// Parses the canonical text strictly: lowercase, exactly six fields, no surrounding
    /// space.
    ///
    /// # Errors
    ///
    /// [`IdentityError`] for any deviation.
    pub fn parse_canonical(text: &str) -> Result<Self, IdentityError> {
        let fields: Vec<&str> = text.split(':').collect();
        let &[
            version,
            fingerprint,
            boot_id,
            generation,
            config_hash,
            route_state_hash,
        ] = fields.as_slice()
        else {
            return Err(IdentityError("field count"));
        };
        if version != "v1" {
            return Err(IdentityError("version"));
        }
        let fingerprint: Hash32 = fingerprint
            .parse()
            .map_err(|_| IdentityError("fingerprint"))?;
        let generation = parse_uint(generation).map_err(|_| IdentityError("generation"))?;
        Ok(Self {
            fingerprint: fingerprint.into(),
            boot_id: BootId::parse(boot_id).map_err(|_| IdentityError("boot_id"))?,
            generation,
            config_hash: config_hash
                .parse()
                .map_err(|_| IdentityError("config_hash"))?,
            route_state_hash: route_state_hash
                .parse()
                .map_err(|_| IdentityError("route_state_hash"))?,
        })
    }
}

/// How a stored identity relates to the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityMatch {
    /// All five fields are equal.
    Exact,
    /// Only the boot id differs: the same helper, generation and sets after a reboot.
    BootOnlyDiffers,
    /// Anything else differs.
    Different,
}

/// Compares a stored identity with the current one.
#[must_use]
pub fn compare(stored: &SafetyIdentity, current: &SafetyIdentity) -> IdentityMatch {
    let same_set = stored.fingerprint == current.fingerprint
        && stored.generation == current.generation
        && stored.config_hash == current.config_hash
        && stored.route_state_hash == current.route_state_hash;
    if !same_set {
        IdentityMatch::Different
    } else if stored.boot_id == current.boot_id {
        IdentityMatch::Exact
    } else {
        IdentityMatch::BootOnlyDiffers
    }
}
