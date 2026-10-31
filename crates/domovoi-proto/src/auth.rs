//! The bytes an agent signs to prove it holds its key.

use domovoi_core::AgentId;

use crate::bytes::Nonce;

/// Domain separator at the start of every challenge message.
pub const CHALLENGE_CONTEXT: &[u8] = b"domovoi-agent-auth-v1\0";

/// The message an agent signs in reply to a `challenge`:
/// `b"domovoi-agent-auth-v1\0" || nonce (32) || agent_id (16, UUID bytes) || session_hint`.
///
/// `session_hint` binds the signature to this connection (for example the TLS channel
/// binding). Signing and verification happen in the runtimes; this fixes the layout.
#[must_use]
pub fn challenge_message(nonce: &Nonce, agent_id: AgentId, session_hint: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(CHALLENGE_CONTEXT.len() + 32 + 16 + session_hint.len());
    out.extend_from_slice(CHALLENGE_CONTEXT);
    out.extend_from_slice(nonce.as_bytes());
    out.extend_from_slice(agent_id.as_uuid().as_bytes());
    out.extend_from_slice(session_hint.as_bytes());
    out
}
