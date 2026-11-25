//! The helper's socket protocol: groups, requests, answers and framing.
//!
//! Pure types. The protocol number here is the helper socket's own and is unrelated to the
//! agent-to-hub wire version. One request is read and one answer written per connection.

mod attestation;
mod frame;
mod group;
pub mod read;
mod request;
mod response;
#[cfg(test)]
mod tests;

pub use attestation::{SignedAttestation, UnsignedToken};
pub use frame::{FrameError, decode_request, decode_response, encode_frame, frame_len};
pub use group::{HelperGroup, ReadGroup, write_tier_gate};
pub use request::{HelperRequest, HelperRequestFrame, Nonce, NonceError};
pub use response::{HelperResponse, HelperResponseFrame, UnavailableReason};

/// This socket's protocol number.
pub const HELPER_PROTOCOL: u16 = 1;
/// The most bytes a request frame may announce.
pub const MAX_REQUEST_BYTES: usize = 65_536;
/// The most bytes an answer frame may announce. Read answers (a container list, a SMART report,
/// 256 KiB of logs) and a signed report as hex all fit; the helper cuts a payload rather than
/// send a larger frame.
pub const MAX_RESPONSE_BYTES: usize = 4_194_304;
/// Where the socket unit listens.
pub const HELPER_SOCKET: &str = "/run/domovoi-helper.sock";

/// The `op` strings a request may carry: the closed set that [`decode_request`] distinguishes
/// from an unknown operation.
pub(crate) const OPERATIONS: &[&str] = &[
    "health",
    "groups",
    "docker_probe",
    "docker_list",
    "docker_events",
    "docker_inspect",
    "docker_logs",
    "smart_scan",
    "smart_report",
    "kernel_log",
    "net_listeners",
    "safety_attest",
    "safety_public_key",
    "unit_identity",
    "unit_restart",
];
