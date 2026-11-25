//! Domain-separated hashes, the signing input and the strict 32-byte hash type.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use super::SIGNING_PREFIX;

/// Why text is not a [`Hash32`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a hash is exactly 64 lowercase hex characters")]
pub struct HashError;

/// A 32-byte hash that is exactly 64 lowercase hex characters on the wire.
///
/// Unlike the hex decoder behind the other hash newtypes, the decoder here refuses uppercase
/// digits and any other length, so one value has one spelling.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash32([u8; 32]);

/// The all-zero hash an incomplete report carries in place of both hashes.
pub const ZERO_HASH: Hash32 = Hash32([0; 32]);

impl Hash32 {
    /// A hash from its raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether this is [`ZERO_HASH`].
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 32]
    }

    /// The SHA-256 of `input`.
    #[must_use]
    pub fn of(input: &[u8]) -> Self {
        Self(Sha256::digest(input).into())
    }
}

impl FromStr for Hash32 {
    type Err = HashError;

    fn from_str(text: &str) -> Result<Self, HashError> {
        let lowercase_hex =
            text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !lowercase_hex {
            return Err(HashError);
        }
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(text, &mut bytes).map_err(|_| HashError)?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash32({self})")
    }
}

impl Serialize for Hash32 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hash32 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = std::borrow::Cow::<str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// The exact bytes that are hashed: `prefix`, then each line followed by LF.
///
/// The prefix constants already end in LF, so nothing is added after the prefix.
#[must_use]
pub fn domain_bytes(prefix: &str, lines: &[String]) -> Vec<u8> {
    let mut out = Vec::from(prefix.as_bytes());
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

/// The SHA-256 of [`domain_bytes`].
#[must_use]
pub fn domain_hash(prefix: &str, lines: &[String]) -> Hash32 {
    Hash32::of(&domain_bytes(prefix, lines))
}

/// The bytes a helper signs and the hub verifies: [`SIGNING_PREFIX`] followed by the whole
/// report.
#[must_use]
pub fn signing_bytes(report: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(SIGNING_PREFIX.len() + report.len());
    out.extend_from_slice(SIGNING_PREFIX.as_bytes());
    out.extend_from_slice(report);
    out
}
