//! Ed25519 attestation keys: encoding, fingerprint, domain-separated signing, strict verification.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::hash::{SIGNATURE_PREFIX, sha256};

/// Why a wire key or signature was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// Not canonical standard padded base64.
    #[error("invalid base64")]
    Base64,
    /// Decoded to the wrong number of bytes.
    #[error("expected {expected} bytes, got {got}")]
    Length {
        /// Required length.
        expected: usize,
        /// Actual length.
        got: usize,
    },
    /// The 32 bytes are not a valid Ed25519 point.
    #[error("not a valid ed25519 public key")]
    Point,
}

/// Fresh key from the cryptographic RNG, the way the repository's agent does it.
#[must_use]
pub fn generate_key() -> SigningKey {
    SigningKey::from_bytes(&rand::random::<[u8; 32]>())
}

/// Public key bytes of a signing key.
#[must_use]
pub fn public_bytes(k: &SigningKey) -> [u8; 32] {
    k.verifying_key().to_bytes()
}

/// Standard padded base64.
#[must_use]
pub fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Strict base64 decode: canonical standard padded form only (re-encoding must match).
///
/// # Errors
/// [`KeyError::Base64`].
pub fn b64_decode(s: &str) -> Result<Vec<u8>, KeyError> {
    let v = STANDARD.decode(s).map_err(|_| KeyError::Base64)?;
    if STANDARD.encode(&v) != s {
        return Err(KeyError::Base64);
    }
    Ok(v)
}

/// Decode exactly `N` bytes from strict base64.
///
/// # Errors
/// [`KeyError::Base64`] or [`KeyError::Length`].
pub fn b64_fixed<const N: usize>(s: &str) -> Result<[u8; N], KeyError> {
    let v = b64_decode(s)?;
    let got = v.len();
    v.try_into().map_err(|_| KeyError::Length { expected: N, got })
}

/// Lowercase hex SHA-256 of the 32 raw public-key bytes.
#[must_use]
pub fn fingerprint(public: &[u8; 32]) -> String {
    hex::encode(sha256(public))
}

/// The label: first 16 hex characters of the fingerprint. Never a verification input.
#[must_use]
pub fn key_id(public: &[u8; 32]) -> String {
    fingerprint(public)[..16].to_owned()
}

/// Display form of a fingerprint: groups of four separated by spaces.
#[must_use]
pub fn group4(fp: &str) -> String {
    fp.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `domovoi-safety-attest-v1\n` followed by the report bytes.
#[must_use]
pub fn signing_bytes(report: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(SIGNATURE_PREFIX.len() + 1 + report.len());
    v.extend_from_slice(SIGNATURE_PREFIX.as_bytes());
    v.push(b'\n');
    v.extend_from_slice(report);
    v
}

/// Sign the report bytes with domain separation.
#[must_use]
pub fn sign_report(key: &SigningKey, report: &[u8]) -> [u8; 64] {
    key.sign(&signing_bytes(report)).to_bytes()
}

/// `verify_strict` over the domain-separated bytes.
///
/// # Errors
/// [`KeyError::Point`] for an invalid key; `Ok(false)` for a bad signature.
pub fn verify_report(public: &[u8; 32], report: &[u8], sig: &[u8; 64]) -> Result<bool, KeyError> {
    let vk = VerifyingKey::from_bytes(public).map_err(|_| KeyError::Point)?;
    Ok(vk.verify_strict(&signing_bytes(report), &Signature::from_bytes(sig)).is_ok())
}
