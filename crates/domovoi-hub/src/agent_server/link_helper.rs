//! The helper half of [`WsAgentLink`] and of the registry: relaying an attestation request and
//! reading what a session says about its helper.

use domovoi_core::HostId;
use domovoi_core::attest::Nonce32;
use domovoi_core::ports::{AttestationReply, HelperLive, HelperReport, LinkError};
use domovoi_proto::Message;
use domovoi_proto::messages::{AttestSafety, HelperStatus};
use jiff::Timestamp;

use super::link::{WsAgentLink, unexpected};
use super::registry::Registry;

impl WsAgentLink {
    /// Asks the host's agent to relay a signed safety report bound to `nonce`.
    ///
    /// A signed answer, an `error` frame (its code is the unsigned token) and `unsupported`
    /// are answers; a timeout is [`LinkError::Timeout`] and changes nothing.
    pub(super) async fn attestation(
        &self,
        host: HostId,
        nonce: Nonce32,
        deadline: Timestamp,
    ) -> Result<AttestationReply, LinkError> {
        let message = Message::AttestSafety(AttestSafety {
            nonce: domovoi_proto::Nonce(*nonce.as_bytes()),
        });
        match self.request(host, &message, deadline).await? {
            Message::SafetyAttestation(answer) => Ok(AttestationReply::Signed {
                public_key: *answer.public_key.as_bytes(),
                report: answer.report.0,
                signature: *answer.signature.as_bytes(),
            }),
            Message::Error(err) => Ok(AttestationReply::Unsigned { token: err.code }),
            Message::Unsupported(_) => Ok(AttestationReply::Unsigned {
                token: "agent_unsupported".to_owned(),
            }),
            other => Err(unexpected(&other)),
        }
    }
}

impl Registry {
    /// What the host's live session says about its helper: the last `agent_status` when there
    /// is one, otherwise the handshake. `None` without a live session.
    pub fn helper_report(&self, host: HostId) -> Option<HelperReport> {
        let hosts = self.lock();
        let live = hosts.get(&host)?.live.as_ref()?;
        let (status, key) = match &live.agent_status {
            Some(status) => (
                &status.helper_status,
                status
                    .helper_identity
                    .as_ref()
                    .map(|identity| *identity.public_key.as_bytes()),
            ),
            None => (&live.helper_status, live.helper_identity),
        };
        let state = match status {
            HelperStatus::Absent => HelperLive::Absent,
            HelperStatus::Ok { ops } => HelperLive::Ok { ops: ops.clone() },
            HelperStatus::Error { reason } => HelperLive::Error {
                reason: reason.clone(),
            },
        };
        Some(HelperReport {
            live: state,
            public_key: key,
        })
    }
}
