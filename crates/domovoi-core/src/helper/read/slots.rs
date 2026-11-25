//! Validated request slots. A slot holds a string privately and is built only through `parse`,
//! so a handler never sees an unchecked value.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why text is not a slot value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SlotError {
    /// Not a Docker container name or id.
    #[error("not a container name or id")]
    Container,
    /// Not a `/dev/` device path.
    #[error("not a device path")]
    Device,
}

/// A Docker container name or id. It starts with a letter or digit, continues with letters,
/// digits, `_`, `.` or `-`, and is at most 128 bytes, so it cannot alter a URL path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContainerRef(String);

impl ContainerRef {
    /// The longest accepted name.
    pub const MAX_LEN: usize = 128;

    /// Parses a container name or id.
    pub fn parse(text: &str) -> Result<Self, SlotError> {
        let bytes = text.as_bytes();
        let first_ok = bytes.first().is_some_and(u8::is_ascii_alphanumeric);
        let rest_ok = bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
        if first_ok && rest_ok && bytes.len() <= Self::MAX_LEN {
            Ok(Self(text.to_owned()))
        } else {
            Err(SlotError::Container)
        }
    }

    /// The validated text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A block device path: `/dev/` then 1 to 32 lowercase letters or digits. This is only a
/// grammar; the helper checks the device against its allowlist separately.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceRef(String);

impl DeviceRef {
    /// Parses a device path.
    pub fn parse(text: &str) -> Result<Self, SlotError> {
        let name = text.strip_prefix("/dev/").ok_or(SlotError::Device)?;
        let ok = (1..=32).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(SlotError::Device)
        }
    }

    /// The validated text, which starts with `/dev/`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! slot_serde {
    ($name:ident) => {
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = std::borrow::Cow::<str>::deserialize(deserializer)?;
                Self::parse(&text).map_err(serde::de::Error::custom)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

slot_serde!(ContainerRef);
slot_serde!(DeviceRef);
