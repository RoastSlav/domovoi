//! Enrollment: trading a one-time token for an agent identity and the hub's pin.
//!
//! The agent dials the hub, records the SHA-256 of the certificate it presents and, when
//! the operator gave an expected fingerprint, refuses any other certificate before sending
//! anything. It then sends `enroll` with a fresh ed25519 public key, and saves the identity
//! the hub returns together with the observed fingerprint.

use std::path::Path;
use std::sync::Arc;

use domovoi_config::AgentConfig;
use domovoi_core::ports::{Clock, HostFs};
use domovoi_proto::messages::{Enroll, RejectReason};
use domovoi_proto::{EnrollToken, Message};
use jiff::SignedDuration;
use url::Url;

use crate::error::{EnrollError, LinkError};
use crate::facts::host_facts;
use crate::identity::{Identity, generate_key};
use crate::link::frame;
use crate::link::tls::RecordingVerifier;
use crate::link::ws::dial;
use crate::time::within;

/// How long the hub may take to answer `enroll`.
pub const ENROLL_TIMEOUT: SignedDuration = SignedDuration::from_secs(30);

/// What enrollment reads besides the hub's answer.
pub struct EnrollEnv<'a> {
    /// Time for the connect and answer timeouts.
    pub clock: &'a dyn Clock,
    /// The host's files, for the facts sent with `enroll`.
    pub fs: &'a dyn HostFs,
}

/// Enrolls with the hub at `hub_url` and saves the identity under `state_dir`.
///
/// With `expected_fingerprint`, a hub presenting any other certificate is refused before
/// the token leaves this host.
pub async fn enroll(
    hub_url: &Url,
    token: EnrollToken,
    expected_fingerprint: Option<[u8; 32]>,
    config: &AgentConfig,
    state_dir: &Path,
    env: EnrollEnv<'_>,
) -> Result<Identity, EnrollError> {
    let verifier = Arc::new(RecordingVerifier::new(expected_fingerprint));
    let mut connected = dial(hub_url, verifier.clone(), env.clock).await?;
    let seen = verifier
        .seen()
        .ok_or_else(|| LinkError::Connect("the hub presented no certificate".to_owned()))?;
    let key = generate_key();
    let request = Enroll {
        token,
        public_key: domovoi_proto::PublicKey(key.verifying_key().to_bytes()),
        host_name: config.agent.host_name.clone(),
        facts: host_facts(env.fs).await,
        agent_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let transport = connected.transport.as_mut();
    frame::send(transport, &Message::Enroll(request), None).await?;
    let answer = within(env.clock, ENROLL_TIMEOUT, frame::expect(transport))
        .await
        .ok_or(LinkError::Timeout)??;
    match answer.message {
        Message::Enrolled(enrolled) => {
            if enrolled.hub_cert_sha256.0 != seen {
                return Err(EnrollError::FingerprintMismatch);
            }
            let identity = Identity::new(enrolled.agent_id, enrolled.host_id, key, seen);
            identity.save(state_dir)?;
            tracing::info!(agent_id = %identity.agent_id, fingerprint = %hex::encode(seen), "enrolled");
            Ok(identity)
        }
        Message::Reject(reject) if reject.reason == RejectReason::BadToken => {
            Err(EnrollError::BadToken)
        }
        Message::Reject(reject) => Err(EnrollError::Rejected(format!("{:?}", reject.reason))),
        Message::Error(err) => Err(EnrollError::Rejected(err.message)),
        other => {
            Err(LinkError::Protocol(format!("expected enrolled, got {}", other.kind())).into())
        }
    }
}
