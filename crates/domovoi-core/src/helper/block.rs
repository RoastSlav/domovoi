//! The target text of a containment request, as it travels on the helper socket.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The longest a network text can be: an IPv6 address with a prefix length.
const MAX_LEN: usize = 49;

/// A network as text. Only the shape is checked here (short, printable ASCII, no space); the
/// helper parses it strictly with `Net::parse` and answers `invalid_target` or `mapped_form`, so
/// those tokens exist for every bad input instead of one undecodable frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSource(String);

/// Why text is not a [`BlockSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a source is at most {MAX_LEN} printable ASCII characters with no space")]
pub struct BlockSourceError;

impl BlockSource {
    /// Wraps `text` after the shape check.
    pub fn new(text: &str) -> Result<Self, BlockSourceError> {
        let ok =
            !text.is_empty() && text.len() <= MAX_LEN && text.bytes().all(|b| b.is_ascii_graphic());
        if ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(BlockSourceError)
        }
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for BlockSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BlockSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = std::borrow::Cow::<str>::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}
