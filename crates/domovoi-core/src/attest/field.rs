//! The field codec (strict percent-encoding) every free-form report value goes through.

/// Why a field, a typed value or a whole line failed the strict grammar.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// A raw byte outside the literal set that is not `%`.
    #[error("raw byte 0x{0:02X} outside the literal set")]
    RawByte(u8),
    /// A `%` with fewer than two following bytes.
    #[error("truncated percent escape")]
    TruncatedEscape,
    /// A `%` followed by something that is not two hex digits.
    #[error("percent escape without two hex digits: %{0}")]
    BadHex(String),
    /// A `%` escape that uses lowercase hex.
    #[error("lowercase hex in percent escape: %{0}")]
    LowercaseHex(String),
    /// A `%` escape of a byte that must appear literally.
    #[error("escape %{0} encodes a byte of the literal set")]
    EncodedLiteral(String),
    /// A line value with the wrong number of `|` separated fields.
    #[error("expected {expected} fields, found {found}")]
    FieldCount {
        /// Fields the key defines.
        expected: usize,
        /// Fields present.
        found: usize,
    },
    /// A typed value that is not canonical.
    #[error("not a canonical {kind}: {value:?}")]
    NotCanonical {
        /// Which type was expected.
        kind: &'static str,
        /// The offending text.
        value: String,
    },
}

/// Builds the "not canonical" error for `value`.
pub(crate) fn not_canonical(kind: &'static str, value: &str) -> EncodeError {
    EncodeError::NotCanonical {
        kind,
        value: value.to_owned(),
    }
}

/// Whether `byte` appears literally in a field: `A-Z a-z 0-9 . _ : / @ -`.
#[must_use]
pub fn is_literal(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'@' | b'-')
}

/// Encodes `bytes`: literal bytes as they are, every other byte as uppercase `%XX`.
#[must_use]
pub fn encode_field(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &byte in bytes {
        if is_literal(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decodes one field strictly.
///
/// # Errors
///
/// Refuses lowercase hex, an escape of a literal byte, a truncated or malformed escape and any
/// raw byte outside the literal set and `%`, reporting the first violation from the left.
pub fn decode_field(text: &str) -> Result<Vec<u8>, EncodeError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            let digits = bytes
                .get(index + 1..index + 3)
                .ok_or(EncodeError::TruncatedEscape)?;
            let pair = String::from_utf8_lossy(digits).into_owned();
            if !digits.iter().all(u8::is_ascii_hexdigit) {
                return Err(EncodeError::BadHex(pair));
            }
            if digits.iter().any(u8::is_ascii_lowercase) {
                return Err(EncodeError::LowercaseHex(pair));
            }
            let value =
                u8::from_str_radix(&pair, 16).map_err(|_| EncodeError::BadHex(pair.clone()))?;
            if is_literal(value) {
                return Err(EncodeError::EncodedLiteral(pair));
            }
            out.push(value);
            index += 3;
        } else if is_literal(byte) {
            out.push(byte);
            index += 1;
        } else {
            return Err(EncodeError::RawByte(byte));
        }
    }
    Ok(out)
}

/// Splits a line value into exactly `count` `|` separated fields.
pub(crate) fn split_fields(value: &str, count: usize) -> Result<Vec<&str>, EncodeError> {
    let fields: Vec<&str> = value.split('|').collect();
    if fields.len() == count {
        Ok(fields)
    } else {
        Err(EncodeError::FieldCount {
            expected: count,
            found: fields.len(),
        })
    }
}
