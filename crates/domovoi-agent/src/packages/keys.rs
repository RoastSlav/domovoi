//! The keys this host trusts for packages, fixed when the agent starts.
//!
//! A [`KeySet`] is built once from `[packages]` in `agent.toml`. It has no way to change
//! afterwards, so nothing the hub sends can add, replace or remove a key.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domovoi_config::PackagesSection;
use domovoi_core::package::{KeyId, KeyState, PackageFailureCode};
use ed25519_dalek::VerifyingKey;

use super::error::PackageError;

/// Trusted public keys and the revocation list.
#[derive(Debug, Clone)]
pub struct KeySet {
    trusted: BTreeMap<KeyId, VerifyingKey>,
    revoked: BTreeSet<KeyId>,
}

impl KeySet {
    /// Builds the set from the configuration.
    ///
    /// # Errors
    /// [`PackageError::InvalidKey`] naming the entry when a key id or revocation is not
    /// 16 lowercase hex characters, a public key is not valid base64 of a valid, non-weak
    /// Ed25519 point, or an `id` differs from the key's own id.
    pub fn from_section(section: &PackagesSection) -> Result<Self, PackageError> {
        let bad = |id: &str, reason: &str| PackageError::InvalidKey {
            id: id.to_owned(),
            reason: reason.to_owned(),
        };
        let mut revoked = BTreeSet::new();
        for entry in &section.revoked {
            let id = KeyId::parse(entry).map_err(|_| bad(entry, "revoked id is not a key id"))?;
            revoked.insert(id);
        }
        let mut trusted = BTreeMap::new();
        for key in &section.trusted_keys {
            let id =
                KeyId::parse(&key.id).map_err(|_| bad(&key.id, "id is not 16 lowercase hex"))?;
            let raw = STANDARD
                .decode(key.public_key.as_bytes())
                .map_err(|_| bad(&key.id, "public_key is not valid base64"))?;
            let bytes: [u8; 32] = raw
                .try_into()
                .map_err(|_| bad(&key.id, "public_key is not 32 bytes"))?;
            let verifying = VerifyingKey::from_bytes(&bytes)
                .map_err(|_| bad(&key.id, "public_key is not a valid Ed25519 point"))?;
            if verifying.is_weak() {
                return Err(bad(&key.id, "public_key is a weak key"));
            }
            if KeyId::of_public_key(&bytes) != id {
                return Err(bad(&key.id, "id does not match the public key"));
            }
            if trusted.insert(id, verifying).is_some() {
                return Err(bad(&key.id, "id is listed twice"));
            }
        }
        Ok(Self { trusted, revoked })
    }

    /// The key for `id`, or why it cannot be used. A revoked id is refused even when it is
    /// also trusted.
    ///
    /// # Errors
    /// `revoked_key` or `untrusted_key`.
    pub fn lookup(&self, id: &KeyId) -> Result<&VerifyingKey, PackageError> {
        if self.revoked.contains(id) {
            return Err(PackageError::new(
                PackageFailureCode::RevokedKey,
                format!("key {id} is revoked on this host"),
            ));
        }
        self.trusted.get(id).ok_or_else(|| {
            PackageError::new(
                PackageFailureCode::UntrustedKey,
                format!("key {id} is not trusted on this host"),
            )
        })
    }

    /// Whether `id` is trusted, unknown or revoked now.
    #[must_use]
    pub fn state(&self, id: &KeyId) -> KeyState {
        if self.revoked.contains(id) {
            KeyState::Revoked
        } else if self.trusted.contains_key(id) {
            KeyState::Trusted
        } else {
            KeyState::Unknown
        }
    }

    /// How many keys are trusted (revoked ones still count until listed in `revoked`).
    #[must_use]
    pub fn trusted_count(&self) -> usize {
        self.trusted.len()
    }
}
