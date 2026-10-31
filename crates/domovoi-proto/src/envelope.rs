//! The JSON envelope around every message, and frame encoding.

use domovoi_core::RequestId;
use serde_json::Value;

use crate::error::ProtoError;
use crate::messages::Message;
use crate::version::PROTOCOL_VERSION;

/// Largest frame either side sends or accepts: 1 MiB.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// One frame on the wire: `{"v", "id", "kind", "reply_to"?, "payload"}`.
///
/// Every frame has a fresh `id`. A response sets `reply_to` to the `id` of the request
/// it answers. Unknown envelope fields are ignored.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Envelope {
    /// The sender's protocol major version.
    pub v: u16,
    /// This frame's id (UUIDv7).
    pub id: RequestId,
    /// The message kind, such as `hello`.
    pub kind: String,
    /// The request this frame answers, if it is a response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<RequestId>,
    /// The message body; its shape depends on `kind`.
    #[serde(default)]
    pub payload: Value,
}

/// A decoded frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// The frame's id.
    pub id: RequestId,
    /// The request it answers, if any.
    pub reply_to: Option<RequestId>,
    /// The message.
    pub message: Message,
}

/// Encodes `message` as one frame with the given `id` and `reply_to`.
///
/// Fails with [`ProtoError::FrameTooLarge`] rather than produce a frame the peer would
/// refuse.
pub fn encode(
    message: &Message,
    id: RequestId,
    reply_to: Option<RequestId>,
) -> Result<String, ProtoError> {
    let payload = message
        .payload()
        .map_err(|err| ProtoError::Encode(err.to_string()))?;
    let envelope = Envelope {
        v: PROTOCOL_VERSION.major,
        id,
        kind: message.kind().to_owned(),
        reply_to,
        payload,
    };
    let frame =
        serde_json::to_string(&envelope).map_err(|err| ProtoError::Encode(err.to_string()))?;
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::FrameTooLarge { len: frame.len() });
    }
    Ok(frame)
}

/// Decodes one frame.
///
/// Checks, in order: the size limit, the envelope shape, the major version, then the
/// payload of known kinds. An unknown kind is not an error: it decodes to
/// [`Message::Unknown`] so the receiver can reply `unsupported`.
pub fn decode(frame: &str) -> Result<Decoded, ProtoError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ProtoError::FrameTooLarge { len: frame.len() });
    }
    let envelope: Envelope =
        serde_json::from_str(frame).map_err(|err| ProtoError::Malformed(err.to_string()))?;
    if envelope.v != PROTOCOL_VERSION.major {
        return Err(ProtoError::UnsupportedMajor { v: envelope.v });
    }
    let message = Message::from_payload(&envelope.kind, envelope.payload).map_err(|err| {
        ProtoError::InvalidPayload {
            kind: envelope.kind.clone(),
            reason: err.to_string(),
        }
    })?;
    Ok(Decoded {
        id: envelope.id,
        reply_to: envelope.reply_to,
        message,
    })
}
