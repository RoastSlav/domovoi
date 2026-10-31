//! Column encodings: timestamps as unix milliseconds, enums as their serde names,
//! structured values as JSON, hashes as 32-byte blobs, and keyset cursors.

use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domovoi_core::ports::StoreError;
use jiff::Timestamp;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::corrupt;

/// A timestamp as unix milliseconds; anything below a millisecond is dropped.
#[must_use]
pub fn ms(at: Timestamp) -> i64 {
    at.as_millisecond()
}

/// A timestamp from unix milliseconds.
pub fn from_ms(value: i64) -> Result<Timestamp, StoreError> {
    Timestamp::from_millisecond(value).map_err(|e| corrupt("timestamp", e))
}

/// An optional timestamp from unix milliseconds.
pub fn from_ms_opt(value: Option<i64>) -> Result<Option<Timestamp>, StoreError> {
    value.map(from_ms).transpose()
}

/// A value as JSON text.
pub fn json<T: Serialize + ?Sized>(value: &T) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|e| StoreError::Backend(format!("cannot encode: {e}")))
}

/// A value from JSON text.
pub fn from_json<T: DeserializeOwned>(text: &str, what: &str) -> Result<T, StoreError> {
    serde_json::from_str(text).map_err(|e| corrupt(what, e))
}

/// The serde name of a value that serializes as a string (a unit enum or a hash).
pub fn name<T: Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        Ok(other) => Err(StoreError::Backend(format!("{other} is not a string"))),
        Err(e) => Err(StoreError::Backend(format!("cannot encode: {e}"))),
    }
}

/// A value from its serde name.
pub fn from_name<T: DeserializeOwned>(text: &str, what: &str) -> Result<T, StoreError> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(|e| corrupt(what, e))
}

/// An id or other `FromStr` value from text.
pub fn parse<T>(text: &str, what: &str) -> Result<T, StoreError>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    text.parse().map_err(|e| corrupt(what, e))
}

/// A 32-byte hash from a blob.
pub fn hash32(bytes: &[u8], what: &str) -> Result<[u8; 32], StoreError> {
    bytes
        .try_into()
        .map_err(|_| corrupt(what, format!("{} bytes, not 32", bytes.len())))
}

/// Saturates a count into SQLite's signed integer.
#[must_use]
pub fn int(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Reads back a count stored with [`int`].
#[must_use]
pub fn uint(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// A keyset position: newest-first listings continue strictly after `(key, id)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// The sort key: milliseconds, or a sequence number.
    pub key: i64,
    /// The tie-breaking id; empty when the key is unique.
    pub id: String,
}

impl Cursor {
    /// The opaque text handed to callers.
    #[must_use]
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(format!("{}:{}", self.key, self.id))
    }

    /// Reads a cursor from [`Cursor::encode`]; anything else is an invalid cursor.
    pub fn decode(text: &str) -> Result<Self, StoreError> {
        let invalid = || StoreError::Backend("invalid cursor".to_owned());
        let raw = URL_SAFE_NO_PAD.decode(text).map_err(|_| invalid())?;
        let raw = String::from_utf8(raw).map_err(|_| invalid())?;
        let (key, id) = raw.split_once(':').ok_or_else(invalid)?;
        Ok(Self {
            key: key.parse().map_err(|_| invalid())?,
            id: id.to_owned(),
        })
    }

    /// Decodes an optional cursor into its bind values.
    pub fn bind(text: Option<&str>) -> Result<(Option<i64>, Option<String>), StoreError> {
        match text {
            Some(text) => {
                let cursor = Self::decode(text)?;
                Ok((Some(cursor.key), Some(cursor.id)))
            }
            None => Ok((None, None)),
        }
    }
}

/// Splits rows fetched with `LIMIT limit + 1` into one page and the cursor that continues
/// after it, `None` on the last page.
pub fn paginate<R>(
    mut rows: Vec<R>,
    limit: usize,
    key: impl Fn(&R) -> Cursor,
) -> (Vec<R>, Option<String>) {
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next = if more {
        rows.last().map(|r| key(r).encode())
    } else {
        None
    };
    (rows, next)
}

/// The `LIMIT` that fetches one row more than a page, to tell whether another page exists.
#[must_use]
pub fn fetch_limit(limit: usize) -> i64 {
    i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_and_reject_garbage() {
        let cursor = Cursor {
            key: 1_700_000_000_123,
            id: "0190a0a0-0000-7000-8000-000000000001".to_owned(),
        };
        assert_eq!(Cursor::decode(&cursor.encode()).unwrap(), cursor);
        assert!(Cursor::decode("not base64!").is_err());
        assert!(Cursor::decode(&URL_SAFE_NO_PAD.encode("nocolon")).is_err());
    }
}
