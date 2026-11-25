//! The boot id and the nonce a report is bound to.

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::helper::Nonce;

/// Why text is not a [`BootId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a boot id is lowercase hex in the 8-4-4-4-12 form")]
pub struct BootIdError;

/// The kernel's boot id in its `8-4-4-4-12` form with lowercase hex only (36 bytes).
///
/// A uuid parser would also accept uppercase, hyphenless and braced forms, so this type
/// validates the text itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BootId(String);

impl BootId {
    /// Parses the strict text form.
    ///
    /// # Errors
    ///
    /// [`BootIdError`] for uppercase, hyphenless, braced or otherwise malformed text.
    pub fn parse(text: &str) -> Result<Self, BootIdError> {
        let ok = text.len() == 36
            && text.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    matches!(byte, b'0'..=b'9' | b'a'..=b'f')
                }
            });
        if ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(BootIdError)
        }
    }

    /// The text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::str::FromStr for BootId {
    type Err = BootIdError;

    fn from_str(text: &str) -> Result<Self, BootIdError> {
        Self::parse(text)
    }
}

impl Serialize for BootId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BootId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = std::borrow::Cow::<str>::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// The hub's challenge as the report carries it: 32 random bytes, 64 lowercase hex characters
/// in text. It converts to and from the helper socket's [`Nonce`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Nonce32([u8; 32]);

impl Nonce32 {
    /// A nonce from its raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parses 64 lowercase hex characters; `None` for uppercase digits or any other length.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Nonce::parse(text).ok().map(Self::from)
    }
}

impl fmt::Display for Nonce32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Nonce32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Nonce32({self})")
    }
}

impl From<Nonce> for Nonce32 {
    fn from(nonce: Nonce) -> Self {
        Self(*nonce.as_bytes())
    }
}

impl From<Nonce32> for Nonce {
    fn from(nonce: Nonce32) -> Self {
        Self::from_bytes(nonce.0)
    }
}
