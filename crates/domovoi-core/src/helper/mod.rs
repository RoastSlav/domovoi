//! The helper's socket protocol: groups, requests, answers and framing.
//!
//! Pure types. The protocol number here is the helper socket's own and is unrelated to the
//! agent-to-hub wire version. One request is read and one answer written per connection.

mod frame;
mod group;
mod request;
mod response;
#[cfg(test)]
mod tests;

pub use frame::{FrameError, decode_request, decode_response, encode_frame, frame_len};
pub use group::{HelperGroup, ReadGroup};
pub use request::{HelperRequest, HelperRequestFrame, Nonce, NonceError};
pub use response::{HelperResponse, HelperResponseFrame, UnavailableReason};

/// This socket's protocol number.
pub const HELPER_PROTOCOL: u16 = 1;
/// The most bytes a request frame may announce.
pub const MAX_FRAME_BYTES: usize = 65_536;
/// Where the socket unit listens.
pub const HELPER_SOCKET: &str = "/run/domovoi-helper.sock";

/// The `op` strings a request may carry: the closed set that [`decode_request`] distinguishes
/// from an unknown operation.
pub(crate) const OPERATIONS: &[&str] = &["health", "groups", "read", "safety_attest"];
