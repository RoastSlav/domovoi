//! The owner keys and revocation list from `[packages]`, decoded once when the hub starts.
//!
//! A [`PackageKeys`] has no way to change after it is built, and nothing but `hub.toml` feeds
//! it: the web settings API refuses the `packages.owner_keys` and `packages.revoked` paths.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domovoi_config::PackagesHubSection;
use domovoi_core::package::KeyId;
use ed25519_dalek::VerifyingKey;

/// A key in `[packages]` that cannot be used. The hub does not start with it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{entry}: {reason}")]
pub struct PackageKeysError {
    /// The config entry, such as `packages.owner_keys[1] (a1b2c3d4e5f60718)`.
    pub entry: String,
    /// What is wrong with it.
    pub reason: String,
}

/// One accepted owner key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerKey {
    /// The key itself.
    pub key: VerifyingKey,
    /// The owner's note for people, when one is set.
    pub label: Option<String>,
}

/// Owner keys and revoked key ids. A key that is in both is revoked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageKeys {
    owners: BTreeMap<KeyId, OwnerKey>,
    revoked: BTreeSet<KeyId>,
}

impl PackageKeys {
    /// Decodes every key in `section`.
    ///
    /// # Errors
    /// [`PackageKeysError`] naming the first entry whose id is not 16 lowercase hex
    /// characters, whose key is not base64 of a valid, non-weak Ed25519 point, whose id
    /// differs from [`KeyId::of_public_key`] of its key, or that is listed twice.
    pub fn from_config(section: &PackagesHubSection) -> Result<Self, PackageKeysError> {
        let mut revoked = BTreeSet::new();
        for (index, entry) in section.revoked.iter().enumerate() {
            let id = KeyId::parse(entry).map_err(|_| PackageKeysError {
                entry: format!("packages.revoked[{index}] ({entry})"),
                reason: "is not a key id (16 lowercase hex characters)".to_owned(),
            })?;
            revoked.insert(id);
        }
        let mut owners = BTreeMap::new();
        for (index, entry) in section.owner_keys.iter().enumerate() {
            let bad = |reason: &str| PackageKeysError {
                entry: format!("packages.owner_keys[{index}] ({})", entry.id),
                reason: reason.to_owned(),
            };
            let id = KeyId::parse(&entry.id).map_err(|_| bad("id is not 16 lowercase hex"))?;
            let raw = STANDARD
                .decode(entry.public_key.as_bytes())
                .map_err(|_| bad("public_key is not valid base64"))?;
            let bytes: [u8; 32] = raw
                .try_into()
                .map_err(|_| bad("public_key is not 32 bytes"))?;
            let key = VerifyingKey::from_bytes(&bytes)
                .map_err(|_| bad("public_key is not a valid Ed25519 point"))?;
            if key.is_weak() {
                return Err(bad("public_key is a weak key"));
            }
            if KeyId::of_public_key(&bytes) != id {
                return Err(bad("id does not match the public key"));
            }
            let owner = OwnerKey {
                key,
                label: entry.label.clone(),
            };
            if owners.insert(id, owner).is_some() {
                return Err(bad("id is listed twice"));
            }
        }
        Ok(Self { owners, revoked })
    }

    /// The owner key for `id`, unless it is revoked or not an owner key.
    #[must_use]
    pub fn trusted(&self, id: &KeyId) -> Option<&OwnerKey> {
        if self.revoked.contains(id) {
            return None;
        }
        self.owners.get(id)
    }

    /// Whether `id` is on the revocation list, whether or not it is also an owner key.
    #[must_use]
    pub fn is_revoked(&self, id: &KeyId) -> bool {
        self.revoked.contains(id)
    }

    /// Every owner key with its id, revoked or not, in id order.
    pub fn owners(&self) -> impl Iterator<Item = (&KeyId, &OwnerKey)> {
        self.owners.iter()
    }

    /// The revoked ids, in id order.
    pub fn revoked(&self) -> impl Iterator<Item = &KeyId> {
        self.revoked.iter()
    }
}

#[cfg(test)]
mod tests {
    use domovoi_config::TrustedKey;
    use ed25519_dalek::SigningKey;

    use super::*;

    fn entry(seed: u8) -> (TrustedKey, KeyId) {
        let public = SigningKey::from_bytes(&[seed; 32])
            .verifying_key()
            .to_bytes();
        let id = KeyId::of_public_key(&public);
        let key = TrustedKey {
            id: id.to_string(),
            public_key: STANDARD.encode(public),
            label: Some("Owner".to_owned()),
        };
        (key, id)
    }

    fn section(keys: Vec<TrustedKey>, revoked: Vec<String>) -> PackagesHubSection {
        PackagesHubSection {
            owner_keys: keys,
            revoked,
            ..PackagesHubSection::default()
        }
    }

    #[test]
    fn hub_owner_keys_decode_and_revoked_wins() {
        let (key, id) = entry(1);
        let keys = PackageKeys::from_config(&section(vec![key.clone()], Vec::new())).unwrap();
        assert!(keys.trusted(&id).is_some());
        assert_eq!(keys.owners().count(), 1);
        let keys = PackageKeys::from_config(&section(vec![key], vec![id.to_string()])).unwrap();
        assert!(keys.trusted(&id).is_none());
        assert_eq!(keys.revoked().count(), 1);
    }

    #[test]
    fn hub_owner_key_id_must_match_public_key() {
        let (mut key, _) = entry(1);
        key.id = entry(2).1.to_string();
        let err = PackageKeys::from_config(&section(vec![key.clone()], Vec::new())).unwrap_err();
        assert!(err.entry.starts_with("packages.owner_keys[0]"), "{err}");
        assert!(err.entry.contains(&key.id), "{err}");
        assert!(err.reason.contains("does not match"), "{err}");
    }

    #[test]
    fn hub_owner_key_must_be_a_real_key() {
        let (mut key, _) = entry(1);
        key.public_key = "not base64!".to_owned();
        assert!(PackageKeys::from_config(&section(vec![key.clone()], Vec::new())).is_err());
        key.public_key = STANDARD.encode([0u8; 31]);
        let err = PackageKeys::from_config(&section(vec![key], Vec::new())).unwrap_err();
        assert!(err.reason.contains("32 bytes"), "{err}");
        let err =
            PackageKeys::from_config(&section(Vec::new(), vec!["nope".to_owned()])).unwrap_err();
        assert!(err.entry.starts_with("packages.revoked[0]"), "{err}");
    }
}
