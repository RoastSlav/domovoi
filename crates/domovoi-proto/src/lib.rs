//! The hub and agent wire protocol.
//!
//! Defines the JSON envelope, every message kind exchanged over the agent link
//! and the protocol version rules. Pure data: no I/O. See `ARCHITECTURE.md` §8.
//!
//! Frames are JSON text: an [`Envelope`] whose `kind` selects one [`Message`] variant.
//! [`encode`] and [`decode`] convert between the two and enforce [`MAX_FRAME_BYTES`].

mod auth;
mod bytes;
mod envelope;
mod error;
pub mod messages;
mod transport;
mod version;

pub use auth::{CHALLENGE_CONTEXT, challenge_message};
pub use bytes::{CertFingerprint, EnrollToken, Nonce, PublicKey, Sha256Bytes, Signature};
pub use envelope::{Decoded, Envelope, MAX_FRAME_BYTES, decode, encode};
pub use error::ProtoError;
pub use messages::Message;
pub use transport::{Transport, TransportError};
pub use version::{PROTOCOL_VERSION, ProtocolVersion, compatible};
