//! The helper's attestation key.
//!
//! This is one of two modules that make a signing key (the other is `lifecycle`, which only
//! calls into this one): every other module receives a [`Key`] that already exists. The seed is
//! never logged, formatted or returned; only the public key, its fingerprint and signatures
//! leave this type.

use domovoi_core::attest::{Hash32, signing_bytes};
use domovoi_core::host::KeyFingerprint;
use ed25519_dalek::{Signer, SigningKey};

/// The length of an Ed25519 seed, which is the whole content of `attest.key`.
pub const SEED_LEN: usize = 32;

/// How the key file is unusable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyFault {
    /// The file does not exist.
    #[error("the key file does not exist")]
    Missing,
    /// The file is not exactly a 32-byte seed.
    #[error("the key file is not a 32-byte Ed25519 seed")]
    Corrupt,
}

/// The helper's Ed25519 attestation key.
pub struct Key {
    signing: SigningKey,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({})", self.key_id())
    }
}

impl Key {
    /// The key a seed names. The file must hold exactly [`SEED_LEN`] bytes.
    ///
    /// # Errors
    ///
    /// [`KeyFault::Corrupt`] for any other length.
    pub fn from_seed_bytes(bytes: &[u8]) -> Result<Self, KeyFault> {
        let seed: [u8; SEED_LEN] = bytes.try_into().map_err(|_| KeyFault::Corrupt)?;
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    /// A new key from the operating system's random source.
    #[must_use]
    pub fn generate() -> (Self, [u8; SEED_LEN]) {
        let seed: [u8; SEED_LEN] = rand::random();
        (
            Self {
                signing: SigningKey::from_bytes(&seed),
            },
            seed,
        )
    }

    /// The 32 public-key bytes.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// The public key as 64 lowercase hex characters.
    #[must_use]
    pub fn public_key_hex(&self) -> String {
        Hash32::from_bytes(self.public_key()).to_string()
    }

    /// The fingerprint the hub computes from the public key.
    #[must_use]
    pub fn fingerprint(&self) -> KeyFingerprint {
        KeyFingerprint::of_public_key(&self.public_key())
    }

    /// The 16-character label of the key: the first 16 hex characters of its fingerprint. A
    /// label only; nothing verifies or decides by it.
    #[must_use]
    pub fn key_id(&self) -> String {
        key_id_of(&self.fingerprint())
    }

    /// Signs `report` under the attestation domain: the signature covers the signing prefix
    /// followed by the report bytes, so a bare-report signature is not a valid attestation.
    #[must_use]
    pub fn sign_report(&self, report: &[u8]) -> [u8; 64] {
        self.signing.sign(&signing_bytes(report)).to_bytes()
    }
}

/// The 16-character label of a fingerprint.
#[must_use]
pub fn key_id_of(fingerprint: &KeyFingerprint) -> String {
    fingerprint.to_string().chars().take(16).collect()
}

/// A fingerprint as the owner compares it: 64 hex characters in groups of four.
#[must_use]
pub fn fingerprint_groups(fingerprint: &KeyFingerprint) -> String {
    let text = fingerprint.to_string();
    text.as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads and checks `attest.key` in `dir`.
///
/// The file is opened without following a symlink and read in full.
///
/// # Errors
///
/// [`KeyFault::Missing`] when it does not exist and [`KeyFault::Corrupt`] when it cannot be read
/// or is not exactly a seed.
#[cfg(unix)]
pub fn load_key(dir: &std::path::Path) -> Result<Key, KeyFault> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
        .open(dir.join(super::KEY_FILE));
    let file = match file {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(KeyFault::Missing),
        Err(_) => return Err(KeyFault::Corrupt),
    };
    let mut bytes = Vec::with_capacity(SEED_LEN + 1);
    file.take(SEED_LEN as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| KeyFault::Corrupt)?;
    Key::from_seed_bytes(&bytes)
}
