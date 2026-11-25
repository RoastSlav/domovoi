//! The one place a helper safety report is verified.
//!
//! The steps run in a fixed order (see the table in the module documentation of
//! [`super`]): decode, pin, signature over the received bytes, parse, hashes, nonce, then
//! the generation rules. A message that fails a check before the nonce step never consumes
//! the nonce, so a forged reply cannot burn the one an honest helper still owes an answer to.

use domovoi_core::attest::{
    CompleteReport, LatestOutcome, MAX_REPORT_BYTES, PinnedKey, Report, ReportError,
    UnverifiedReason, parse_report, signing_bytes,
};
use domovoi_core::ports::HighWater;
use ed25519_dalek::{Signature, VerifyingKey};
use jiff::Timestamp;

use super::nonce::NonceStore;
use domovoi_core::HostId;

/// What the verifier checks: a reply as the link delivered it, unverified.
pub(crate) struct Received<'a> {
    /// The public key the helper presented.
    pub public_key: &'a [u8; 32],
    /// The report bytes, exactly as relayed.
    pub report: &'a [u8],
    /// The signature over the signing prefix and the report.
    pub signature: &'a [u8; 64],
}

/// A complete report that passed every check. Only [`verify`] builds one.
#[derive(Debug)]
pub(crate) struct VerifiedReport {
    report: CompleteReport,
    bytes: Vec<u8>,
    nonce_issued_at: Timestamp,
}

impl VerifiedReport {
    /// The parsed report.
    pub(crate) fn parsed(&self) -> &CompleteReport {
        &self.report
    }

    /// The received bytes.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// When the hub issued the nonce the report answers.
    pub(crate) fn nonce_issued_at(&self) -> Timestamp {
        self.nonce_issued_at
    }
}

/// What verifying one reply decided.
#[derive(Debug)]
pub(crate) enum Verdict {
    /// Every check passed: store it.
    Accepted(Box<VerifiedReport>),
    /// The reply is refused and nothing about the host's trust changes.
    Rejected {
        /// The wire reason.
        reason: UnverifiedReason,
        /// The internal detail.
        detail: String,
    },
    /// The reply changes the latest outcome and revokes the stored verified state.
    Revoking {
        /// The new latest outcome.
        outcome: LatestOutcome,
        /// The wire reason.
        reason: UnverifiedReason,
        /// The internal detail.
        detail: String,
    },
}

fn reject(reason: UnverifiedReason, detail: &str) -> Verdict {
    Verdict::Rejected {
        reason,
        detail: detail.to_owned(),
    }
}

fn revoke(outcome: LatestOutcome, reason: UnverifiedReason, detail: &str) -> Verdict {
    Verdict::Revoking {
        outcome,
        reason,
        detail: detail.to_owned(),
    }
}

/// Verifies `received` for `host` against the owner's pin, in the fixed order. `high_water`
/// is the stored row for the pinned key; `now` is the hub's clock.
pub(crate) fn verify(
    host: HostId,
    pin: Option<&PinnedKey>,
    received: &Received<'_>,
    nonces: &NonceStore,
    high_water: Option<&HighWater>,
    now: Timestamp,
) -> Verdict {
    // Step 1: decode. The arrays are already the right length; the report has a cap.
    if received.report.len() > MAX_REPORT_BYTES {
        return reject(UnverifiedReason::SignatureInvalid, "bad_encoding");
    }
    // Step 2: the pin is the only trust root.
    let Some(pin) = pin else {
        return reject(UnverifiedReason::KeyNotPinned, "key_not_pinned");
    };
    if received.public_key != &pin.public_key {
        return revoke(
            LatestOutcome::Mismatch,
            UnverifiedReason::KeyMismatch,
            "key_mismatch",
        );
    }
    // Step 3: the signature, over the bytes as received, before anything is parsed.
    if !signature_holds(&pin.public_key, received) {
        return reject(UnverifiedReason::SignatureInvalid, "signature_invalid");
    }
    // Steps 4 and 5: parse the same bytes; a complete report's hashes are recomputed.
    let parsed = match parse_report(received.report) {
        Ok(parsed) => parsed,
        Err(ReportError::BadFormat(_)) => {
            return reject(UnverifiedReason::ReportIncomplete, "format_unsupported");
        }
        Err(_) => return reject(UnverifiedReason::ReportIncomplete, "report_malformed"),
    };
    let nonce = match &parsed {
        Report::Complete(report) => {
            if report.check_hashes().is_err() {
                return reject(UnverifiedReason::ReportIncomplete, "hash_mismatch");
            }
            report.nonce
        }
        Report::Incomplete(report) => report.nonce,
    };
    // Step 6: the nonce is consumed here, late, only for a validly signed, well-formed reply.
    let issued_at = match nonces.take(host, &nonce, now) {
        Ok(issued_at) => issued_at,
        Err(err) => return reject(UnverifiedReason::AttestationMissing, err.detail()),
    };
    let report = match parsed {
        // Step 7: a validly signed incomplete report revokes.
        Report::Incomplete(report) => {
            return revoke(
                LatestOutcome::SignedIncomplete,
                UnverifiedReason::ReportIncomplete,
                &format!("helper_incomplete:{}", report.reason.as_str()),
            );
        }
        Report::Complete(report) => report,
    };
    // Step 8.
    if report.generation == 0 {
        return reject(UnverifiedReason::ReportIncomplete, "generation_zero");
    }
    // Steps 9 and 10: rollback and repeat.
    if let Some(row) = high_water {
        if report.generation < row.generation {
            return revoke(
                LatestOutcome::Regressed,
                UnverifiedReason::GenerationRegressed,
                "generation_regressed",
            );
        }
        if report.generation == row.generation
            && (report.config_hash != row.config_hash
                || report.route_state_hash != row.route_state_hash)
        {
            return revoke(
                LatestOutcome::Regressed,
                UnverifiedReason::GenerationRegressed,
                "generation_repeated",
            );
        }
    }
    Verdict::Accepted(Box::new(VerifiedReport {
        report: *report,
        bytes: received.report.to_vec(),
        nonce_issued_at: issued_at,
    }))
}

/// `verify_strict` of the pinned key over the signing prefix and the received report bytes.
fn signature_holds(pinned: &[u8; 32], received: &Received<'_>) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(pinned) else {
        return false;
    };
    let signature = Signature::from_bytes(received.signature);
    key.verify_strict(&signing_bytes(received.report), &signature)
        .is_ok()
}
