//! Why a frame could not be encoded or decoded.

/// An encoding or decoding failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtoError {
    /// The frame is larger than [`crate::MAX_FRAME_BYTES`].
    #[error("frame of {len} bytes exceeds the {max} byte limit", max = crate::MAX_FRAME_BYTES)]
    FrameTooLarge {
        /// Size of the frame in bytes.
        len: usize,
    },
    /// The frame is not a valid envelope.
    #[error("malformed envelope: {0}")]
    Malformed(String),
    /// The envelope's `v` is not this build's major version.
    #[error("unsupported protocol major version {v}")]
    UnsupportedMajor {
        /// The major version in the envelope.
        v: u16,
    },
    /// A known kind carried a payload that does not match its schema.
    #[error("invalid `{kind}` payload: {reason}")]
    InvalidPayload {
        /// The message kind.
        kind: String,
        /// What is wrong.
        reason: String,
    },
    /// A message could not be serialized.
    #[error("cannot encode message: {0}")]
    Encode(String),
}
