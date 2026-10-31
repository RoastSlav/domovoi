//! Fixed-length byte strings and the enrollment token.
//!
//! Keys, nonces and signatures travel as standard padded base64. Digests travel as
//! lowercase hex, like the plan and manifest hashes from `domovoi-core`.

use std::fmt;

mod base64_codec {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    pub(super) const NAME: &str = "base64";

    pub(super) fn encode(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    pub(super) fn decode(text: &str) -> Result<Vec<u8>, String> {
        STANDARD.decode(text).map_err(|err| err.to_string())
    }
}

mod hex_codec {
    use std::fmt::Write as _;

    pub(super) const NAME: &str = "hex";

    pub(super) fn encode(bytes: &[u8]) -> String {
        bytes
            .iter()
            .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
                let _ = write!(out, "{b:02x}");
                out
            })
    }

    pub(super) fn decode(text: &str) -> Result<Vec<u8>, String> {
        if !text.len().is_multiple_of(2)
            || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err("expected lowercase hex".to_owned());
        }
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|err| err.to_string()))
            .collect()
    }
}

macro_rules! byte_string {
    ($(#[$meta:meta])* $name:ident, $len:literal, $codec:ident) => {
        $(#[$meta])*
        ///
        /// Decoding rejects any other length.
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub [u8; $len]);

        impl $name {
            /// The raw bytes.
            #[must_use]
            pub fn as_bytes(&self) -> &[u8; $len] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), $codec::encode(&self.0))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&$codec::encode(&self.0))
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <String as serde::Deserialize>::deserialize(deserializer)?;
                let bytes = $codec::decode(&text).map_err(serde::de::Error::custom)?;
                let len = bytes.len();
                bytes.try_into().map(Self).map_err(|_| {
                    serde::de::Error::custom(format!(
                        "expected {} bytes of {}, got {len}",
                        $len,
                        $codec::NAME
                    ))
                })
            }
        }
    };
}

byte_string!(
    /// An agent's ed25519 public key.
    PublicKey,
    32,
    base64_codec
);
byte_string!(
    /// A random challenge from the hub.
    Nonce,
    32,
    base64_codec
);
byte_string!(
    /// An ed25519 signature over [`crate::challenge_message`].
    Signature,
    64,
    base64_codec
);
byte_string!(
    /// SHA-256 of the hub's TLS certificate (DER), which the agent pins.
    CertFingerprint,
    32,
    hex_codec
);
byte_string!(
    /// A SHA-256 digest, such as the hash of the argv a skill ran.
    Sha256Bytes,
    32,
    hex_codec
);

/// A single-use enrollment token. `Debug` never shows the value.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EnrollToken(String);

impl EnrollToken {
    /// Wraps a token string.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The token, for hashing and comparison by the hub.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EnrollToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnrollToken([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enroll_token_debug_is_redacted() {
        assert_eq!(
            format!("{:?}", EnrollToken::new("abc")),
            "EnrollToken([REDACTED])"
        );
    }
}
