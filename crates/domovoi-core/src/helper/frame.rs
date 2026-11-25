//! Length-prefixed JSON framing for the helper socket.

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::request::HelperRequestFrame;
use super::response::HelperResponseFrame;
use super::{HELPER_PROTOCOL, MAX_FRAME_BYTES, MAX_RESPONSE_BYTES};
use crate::identity::RefusalToken;

/// Why a frame could not be built or read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The length exceeds [`MAX_FRAME_BYTES`].
    #[error("frame of {0} bytes exceeds the cap")]
    FrameTooLarge(usize),
    /// The value could not be written as JSON.
    #[error("cannot encode frame: {0}")]
    Encode(String),
    /// The body is not the expected JSON.
    #[error("malformed frame: {0}")]
    Malformed(String),
}

impl FrameError {
    /// The token a server answers with for this error.
    #[must_use]
    pub const fn token(&self) -> RefusalToken {
        match self {
            Self::FrameTooLarge(_) => RefusalToken::FrameTooLarge,
            Self::Encode(_) | Self::Malformed(_) => RefusalToken::FrameMalformed,
        }
    }
}

/// Writes `value` as a 4-byte big-endian length followed by UTF-8 JSON.
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(value).map_err(|e| FrameError::Encode(e.to_string()))?;
    let len = u32::try_from(body.len()).map_err(|_| FrameError::FrameTooLarge(body.len()))?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// The body length a header announces, refused above [`MAX_FRAME_BYTES`] before any body is read.
pub fn frame_len(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME_BYTES {
        Err(FrameError::FrameTooLarge(len))
    } else {
        Ok(len)
    }
}

/// The body length an answer header announces, refused above [`MAX_RESPONSE_BYTES`] before any
/// body is read.
pub fn frame_len_response(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_RESPONSE_BYTES {
        Err(FrameError::FrameTooLarge(len))
    } else {
        Ok(len)
    }
}

/// Reads a request body.
///
/// A `v` other than 1 is `unsupported_version`; an `op` outside the closed set is
/// `unknown_operation`; anything else wrong (not JSON, an unknown field, a bad nonce) is
/// `frame_malformed`.
pub fn decode_request(body: &[u8]) -> Result<HelperRequestFrame, RefusalToken> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| RefusalToken::FrameMalformed)?;
    let object = value.as_object().ok_or(RefusalToken::FrameMalformed)?;
    let version = object
        .get("v")
        .and_then(serde_json::Value::as_u64)
        .ok_or(RefusalToken::FrameMalformed)?;
    if version != u64::from(HELPER_PROTOCOL) {
        return Err(RefusalToken::UnsupportedVersion);
    }
    let request = object
        .get("request")
        .and_then(serde_json::Value::as_object)
        .ok_or(RefusalToken::FrameMalformed)?;
    let op = request
        .get("op")
        .and_then(serde_json::Value::as_str)
        .ok_or(RefusalToken::FrameMalformed)?;
    if !super::OPERATIONS.contains(&op) {
        return Err(RefusalToken::UnknownOperation);
    }
    // serde ignores unknown fields on a unit variant of an internally tagged enum, so the
    // closed field set is enforced here as well.
    let field = match op {
        "read" => Some("group"),
        "safety_attest" => Some("nonce"),
        _ => None,
    };
    if request
        .keys()
        .any(|key| key != "op" && Some(key.as_str()) != field)
    {
        return Err(RefusalToken::FrameMalformed);
    }
    serde_json::from_value(value).map_err(|_| RefusalToken::FrameMalformed)
}

/// Reads an answer body.
pub fn decode_response(body: &[u8]) -> Result<HelperResponseFrame, FrameError> {
    decode_json(body)
}

fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(body).map_err(|e| FrameError::Malformed(e.to_string()))
}
