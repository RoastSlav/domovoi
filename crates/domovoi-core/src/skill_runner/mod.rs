//! The agent-to-runner protocol: the frames the control agent and the skill runner exchange.
//!
//! Pure types. The protocol number here is the local socket's own and is unrelated to the
//! agent-to-hub wire version. One request is read and at most one reply written per connection.

mod frame;
mod reply;
mod request;
#[cfg(test)]
mod tests;

pub use frame::{FrameError, decode_payload, encode_frame, frame_len};
pub use reply::{HealthReply, Refusal, RefusalCode, RunReply, RunnerReply};
pub use request::{Header, ProbeRequest, RunRequest, RunnerRequest, SkillSource};

/// Version of the agent-to-runner protocol. Both sides refuse another value.
pub const RUNNER_PROTOCOL: u32 = 1;
/// Path of the runner socket; the only definition of the literal (rule `RUNNER-1`).
pub const RUNNER_SOCKET_PATH: &str = "/run/domovoi-skill-runner.sock";
/// Largest frame either side accepts: two streams of 1 MiB, each byte JSON-escaped to six, plus overhead.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Largest request frame (requests are small).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
