//! The safety attestation answers on the helper socket.
//!
//! A signed attestation travels as lowercase hex strings so that a report of up to 64 KiB fits
//! one answer frame (see [`super::MAX_RESPONSE_BYTES`]); the bytes are never parsed on the way.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why the helper could not sign an attestation at all. These answers carry no signature: they
/// are an unverified hint, never evidence, and can unlock nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsignedToken {
    /// Neither the key nor the state exists: `helper init` has not completed.
    Uninitialized,
    /// The state exists and the key does not. The helper never makes a key by itself.
    KeyMissing,
    /// The key file is not a 32-byte Ed25519 seed.
    KeyCorrupt,
    /// The directory, key, state or lock has a wrong owner, mode or type.
    HelperStatePermissions,
    /// The lock file cannot be opened or locked.
    LockUnavailable,
}

impl UnsignedToken {
    /// Every token, in table order.
    pub const ALL: [Self; 5] = [
        Self::Uninitialized,
        Self::KeyMissing,
        Self::KeyCorrupt,
        Self::HelperStatePermissions,
        Self::LockUnavailable,
    ];

    /// The stable spelling, also used as the `error` code the agent relays to the hub.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::KeyMissing => "key_missing",
            Self::KeyCorrupt => "key_corrupt",
            Self::HelperStatePermissions => "helper_state_permissions",
            Self::LockUnavailable => "lock_unavailable",
        }
    }
}

impl std::fmt::Display for UnsignedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A report and the signature over its signing bytes, with the public key that made it.
///
/// The three byte strings are opaque here. The hub verifies the signature against its pinned
/// key over exactly these bytes; nobody between the helper and the hub rewrites them.
#[derive(Clone, PartialEq, Eq)]
pub struct SignedAttestation {
    /// The Ed25519 public key that signed.
    pub public_key: [u8; 32],
    /// The report text, byte for byte as signed.
    pub report: Vec<u8>,
    /// The Ed25519 signature over `SIGNING_PREFIX` followed by the report.
    pub signature: [u8; 64],
}

impl std::fmt::Debug for SignedAttestation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SignedAttestation {{ report: {} bytes }}",
            self.report.len()
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    public_key: String,
    report: String,
    signature: String,
}

fn lowercase_hex(text: &str) -> bool {
    text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn fixed<const N: usize>(text: &str) -> Result<[u8; N], String> {
    if !lowercase_hex(text) {
        return Err("expected lowercase hex".to_owned());
    }
    let mut out = [0_u8; N];
    hex::decode_to_slice(text, &mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

impl Serialize for SignedAttestation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Wire {
            public_key: hex::encode(self.public_key),
            report: hex::encode(&self.report),
            signature: hex::encode(self.signature),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SignedAttestation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        use serde::de::Error;
        if !lowercase_hex(&wire.report) {
            return Err(D::Error::custom("report: expected lowercase hex"));
        }
        Ok(Self {
            public_key: fixed::<32>(&wire.public_key).map_err(D::Error::custom)?,
            report: hex::decode(&wire.report).map_err(D::Error::custom)?,
            signature: fixed::<64>(&wire.signature).map_err(D::Error::custom)?,
        })
    }
}
