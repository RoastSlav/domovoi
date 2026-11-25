//! `attest_safety`: relays the hub's challenge to the helper and the helper's signed answer back.
//!
//! The agent is a relay and nothing more. It passes the nonce to the helper and the report and
//! the signature back as the byte strings the helper made, without parsing, re-serialising or
//! filtering them: the hub verifies a signature over exactly those bytes, so anything the agent
//! touched would simply fail to verify. A helper that cannot sign is answered with an `error`
//! frame whose code is the helper's own token, so the hub does not wait for a timeout; a helper
//! that is absent, unreachable or not serving the operation is `helper_unavailable`.

use domovoi_core::helper::Nonce;
use domovoi_proto::messages::{AttestSafety, ErrorMessage, SafetyAttestation};
use domovoi_proto::{Base64Bytes, MAX_BASE64_BYTES, Message, PublicKey, Signature};

use super::RequestCtx;
use crate::helper_client::HELPER_UNAVAILABLE;

/// The text of every relayed failure. The hub gates on the `code`, never on this.
const FAILURE_TEXT: &str = "the helper did not return a signed safety report";

fn failure(code: &str) -> Message {
    Message::Error(ErrorMessage {
        code: code.to_owned(),
        message: FAILURE_TEXT.to_owned(),
    })
}

/// Answers one `attest_safety` challenge.
pub async fn attest_reply(ctx: &RequestCtx, challenge: &AttestSafety) -> Message {
    let nonce = Nonce::from_bytes(*challenge.nonce.as_bytes());
    match ctx.helper.attest(nonce, &*ctx.clock).await {
        Ok(attestation) if attestation.report.len() <= MAX_BASE64_BYTES => {
            Message::SafetyAttestation(SafetyAttestation {
                public_key: PublicKey(attestation.public_key),
                report: Base64Bytes(attestation.report),
                signature: Signature(attestation.signature),
            })
        }
        Ok(_) => failure(HELPER_UNAVAILABLE),
        Err(err) => failure(err.relay_code()),
    }
}
