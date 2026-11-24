//! Length-prefixed JSON frames.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Why a frame could not be built or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The announced or encoded length is zero.
    #[error("empty frame")]
    Empty,
    /// The length exceeds the limit of this frame kind.
    #[error("frame of {len} bytes exceeds the limit of {limit}")]
    TooLarge {
        /// The length announced or encoded.
        len: usize,
        /// The limit that applied.
        limit: usize,
    },
    /// The body is not the expected JSON, or the value could not be written.
    #[error("malformed frame: {0}")]
    Malformed(String),
}

/// Writes `value` as a 4-byte big-endian length followed by UTF-8 JSON, refusing a body above `limit`.
///
/// # Errors
/// [`FrameError::TooLarge`] above `limit`, [`FrameError::Malformed`] when `value` cannot be encoded.
pub fn encode_frame<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if body.len() > limit {
        return Err(FrameError::TooLarge {
            len: body.len(),
            limit,
        });
    }
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge {
        len: body.len(),
        limit,
    })?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// The body length a header announces, refused when zero or above `limit` before any body is read.
///
/// # Errors
/// [`FrameError::Empty`] or [`FrameError::TooLarge`].
pub fn frame_len(header: [u8; 4], limit: usize) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > limit {
        return Err(FrameError::TooLarge { len, limit });
    }
    Ok(len)
}

/// Reads a frame body as `T`; unknown fields are refused by the types themselves.
///
/// # Errors
/// [`FrameError::Empty`] for an empty body, [`FrameError::Malformed`] for anything that is not a `T`.
pub fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, FrameError> {
    if payload.is_empty() {
        return Err(FrameError::Empty);
    }
    serde_json::from_slice(payload).map_err(|e| FrameError::Malformed(e.to_string()))
}
