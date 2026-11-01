//! The authentication handshake: `hello`, `challenge`, signed response, `welcome`.

use domovoi_core::SkillSummary;
use domovoi_proto::messages::{
    AgentSettings, ChallengeResponse, CheckInfo, Hello, HostFacts, RejectReason, Welcome,
};
use domovoi_proto::{Message, PROTOCOL_VERSION, Transport, challenge_message};
use jiff::SignedDuration;

use super::frame;
use crate::error::LinkError;
use crate::identity::Identity;

/// How long each handshake step may take.
pub const STEP_TIMEOUT: SignedDuration = SignedDuration::from_secs(30);

/// What the agent announces in `hello`.
pub struct Announce<'a> {
    /// Host facts.
    pub facts: HostFacts,
    /// Summaries of the skills in the local catalogue.
    pub skills: &'a [SkillSummary],
    /// Every check with its local settings.
    pub checks: &'a [CheckInfo],
}

/// Runs the handshake on a fresh connection and returns the hub's `welcome`.
///
/// The challenge signature covers `session_hint` (the TLS channel binding), so it cannot be
/// replayed on another connection.
pub async fn handshake(
    transport: &mut dyn Transport,
    identity: &Identity,
    session_hint: &str,
    announce: Announce<'_>,
) -> Result<Welcome, LinkError> {
    let hello = Hello {
        agent_id: identity.agent_id,
        protocol: PROTOCOL_VERSION,
        agent_version: env!("CARGO_PKG_VERSION").to_owned(),
        facts: announce.facts,
        capabilities: Vec::new(),
        skills: announce.skills.to_vec(),
        checks: announce.checks.to_vec(),
    };
    frame::send(transport, &Message::Hello(hello), None).await?;
    let challenge = match frame::expect(transport).await?.message {
        Message::Challenge(challenge) => challenge,
        other => return Err(refused(other)),
    };
    let signed = challenge_message(&challenge.nonce, identity.agent_id, session_hint);
    let response = ChallengeResponse {
        signature: identity.sign(&signed),
    };
    frame::send(transport, &Message::ChallengeResponse(response), None).await?;
    match frame::expect(transport).await?.message {
        Message::Welcome(welcome) => Ok(welcome),
        other => Err(refused(other)),
    }
}

fn refused(message: Message) -> LinkError {
    match message {
        Message::Reject(reject) => match reject.reason {
            RejectReason::Revoked => LinkError::Revoked("revoked"),
            RejectReason::UnknownAgent => LinkError::Revoked("unknown agent"),
            RejectReason::Incompatible { hub } => LinkError::Incompatible {
                hub,
                agent: PROTOCOL_VERSION,
            },
            other => LinkError::Protocol(format!("rejected: {other:?}")),
        },
        other => LinkError::Protocol(format!("unexpected {} during handshake", other.kind())),
    }
}

/// The heartbeat interval from the hub's settings; zero means the default of 10 s.
#[must_use]
pub fn heartbeat(settings: &AgentSettings) -> SignedDuration {
    match settings.heartbeat_seconds {
        0 => SignedDuration::from_secs(10),
        secs => SignedDuration::from_secs(i64::from(secs)),
    }
}
