//! The first frames of a connection: enrollment, or hello, challenge and welcome.
//!
//! Nothing reaches the session loop, and so the [`AgentLink`](domovoi_core::ports::AgentLink),
//! until the agent has signed a fresh challenge with its enrolled key over this
//! connection's channel binding (INV-10).

use std::collections::BTreeMap;

use domovoi_core::ports::StoreError;
use domovoi_core::{AgentId, AgentIdentity, RequestId};
use domovoi_proto::messages::{
    AgentSettings, Challenge, Enroll, Enrolled, Hello, Reject, RejectReason, SessionId, Welcome,
};
use domovoi_proto::{
    CertFingerprint, Message, Nonce, PROTOCOL_VERSION, Transport, challenge_message, compatible,
};
use ed25519_dalek::{Signature, VerifyingKey};
use jiff::SignedDuration;

use super::Inner;
use super::conn::{ConnError, expect, send, within};
use super::tokens::token_hash;

/// How long a new connection may take to send `enroll` or `hello`.
pub const FIRST_FRAME_TIMEOUT: SignedDuration = SignedDuration::from_secs(10);

/// How long the agent may take to answer the challenge.
pub const STEP_TIMEOUT: SignedDuration = SignedDuration::from_secs(10);

/// How long a wrong token waits before it is refused, to slow down guessing.
pub const BAD_TOKEN_DELAY: SignedDuration = SignedDuration::from_secs(1);

/// An agent that proved who it is.
pub(crate) struct Accepted {
    pub agent: AgentIdentity,
    pub hello: Hello,
    pub session: SessionId,
}

/// How the handshake ended.
pub(crate) enum Shaken {
    /// A new agent enrolled; the connection closes.
    Enrolled,
    /// An enrolled agent authenticated; the session starts.
    Session(Box<Accepted>),
}

/// Runs the handshake on a fresh connection whose channel binding is `binding`.
pub(crate) async fn handshake(
    inner: &Inner,
    transport: &mut dyn Transport,
    binding: &str,
) -> Result<Shaken, ConnError> {
    let clock = &*inner.deps.clock;
    let first = within(clock, FIRST_FRAME_TIMEOUT, expect(transport))
        .await
        .ok_or(ConnError::Timeout)??;
    match first.message {
        Message::Enroll(enroll) => {
            enroll_agent(inner, transport, first.id, enroll).await?;
            Ok(Shaken::Enrolled)
        }
        Message::Hello(hello) => authenticate(inner, transport, first.id, hello, binding)
            .await
            .map(|accepted| Shaken::Session(Box::new(accepted))),
        other => Err(ConnError::Protocol(format!(
            "the first frame must be enroll or hello, not {}",
            other.kind()
        ))),
    }
}

async fn reject(
    transport: &mut dyn Transport,
    reason: RejectReason,
    reply_to: RequestId,
) -> ConnError {
    let message = Message::Reject(Reject { reason });
    // The connection closes either way; a failed send changes nothing.
    let _ = send(transport, &message, Some(reply_to)).await;
    ConnError::Rejected(reason)
}

async fn enroll_agent(
    inner: &Inner,
    transport: &mut dyn Transport,
    id: RequestId,
    enroll: Enroll,
) -> Result<(), ConnError> {
    let clock = &*inner.deps.clock;
    let hosts = &*inner.deps.hosts;
    let now = clock.now();
    // The token is looked up by its SHA-256, so the time taken says nothing about how
    // close a guess was.
    let host = match hosts
        .consume_enrollment_token(&token_hash(enroll.token.expose()), now)
        .await
    {
        Ok(host) => host,
        Err(StoreError::NotFound) => {
            let until = now.saturating_add(BAD_TOKEN_DELAY).unwrap_or(now);
            clock.sleep_until(until).await;
            return Err(reject(transport, RejectReason::BadToken, id).await);
        }
        Err(err) => return Err(err.into()),
    };
    if VerifyingKey::from_bytes(&enroll.public_key.0).is_err() {
        return Err(reject(transport, RejectReason::Unknown, id).await);
    }
    // The token is the operator's say-so for this host: a new enrollment replaces the
    // host's agent, as after a reinstall.
    if let Some(old) = hosts.live_agent(host).await? {
        hosts.revoke_agent(old.agent, now).await?;
        inner.registry.close(host);
        tracing::info!(%host, agent = %old.agent, "re-enrolled; the previous agent is revoked");
    }
    let agent = AgentIdentity {
        agent: AgentId::new(),
        host,
        public_key: enroll.public_key.0,
        version: enroll.agent_version,
        enrolled_at: now,
        revoked_at: None,
    };
    hosts.put_agent(&agent).await?;
    tracing::info!(%host, agent = %agent.agent, "agent enrolled");
    let enrolled = Message::Enrolled(Enrolled {
        agent_id: agent.agent,
        host_id: host,
        hub_cert_sha256: CertFingerprint(inner.tls.fingerprint()),
    });
    send(transport, &enrolled, Some(id)).await
}

async fn authenticate(
    inner: &Inner,
    transport: &mut dyn Transport,
    id: RequestId,
    hello: Hello,
    binding: &str,
) -> Result<Accepted, ConnError> {
    let clock = &*inner.deps.clock;
    let hosts = &*inner.deps.hosts;
    let Some(mut agent) = hosts.get_agent(hello.agent_id).await? else {
        return Err(reject(transport, RejectReason::UnknownAgent, id).await);
    };
    if agent.revoked_at.is_some() {
        return Err(reject(transport, RejectReason::Revoked, id).await);
    }
    if !compatible(hello.protocol, PROTOCOL_VERSION) {
        let reason = RejectReason::Incompatible {
            hub: PROTOCOL_VERSION,
        };
        return Err(reject(transport, reason, id).await);
    }
    let nonce = Nonce(rand::random());
    send(
        transport,
        &Message::Challenge(Challenge { nonce }),
        Some(id),
    )
    .await?;
    let answer = within(clock, STEP_TIMEOUT, expect(transport))
        .await
        .ok_or(ConnError::Timeout)??;
    let Message::ChallengeResponse(response) = answer.message else {
        return Err(reject(transport, RejectReason::BadSignature, answer.id).await);
    };
    let signed = challenge_message(&nonce, agent.agent, binding);
    let valid = VerifyingKey::from_bytes(&agent.public_key).is_ok_and(|key| {
        key.verify_strict(&signed, &Signature::from_bytes(&response.signature.0))
            .is_ok()
    });
    if !valid {
        return Err(reject(transport, RejectReason::BadSignature, answer.id).await);
    }
    if agent.version != hello.agent_version {
        agent.version.clone_from(&hello.agent_version);
        hosts.put_agent(&agent).await?;
    }
    let session = SessionId::new();
    let heartbeat_seconds = u32::try_from(inner.settings.heartbeat.as_secs()).unwrap_or(u32::MAX);
    let welcome = Message::Welcome(Welcome {
        session_id: session,
        hub_version: env!("CARGO_PKG_VERSION").to_owned(),
        config: AgentSettings {
            heartbeat_seconds,
            check_overrides: BTreeMap::new(),
        },
    });
    send(transport, &welcome, Some(answer.id)).await?;
    Ok(Accepted {
        agent,
        hello,
        session,
    })
}
