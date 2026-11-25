//! Committing a verification result: the accepted report with its high-water mark, or a
//! recorded outcome that revokes. Every commit carries its audit rows, none of which holds
//! report, signature or nonce bytes.

use domovoi_core::attest::{LatestOutcome, PinnedKey, SafetyIdentity, UnverifiedReason};
use domovoi_core::ports::{
    Accepted, AuditDraft, CommitError, HelperTrustWrite, HighWater, OutcomeRecord, StateWrite,
    StoredTrust, Transaction,
};
use domovoi_core::{Actor, HostId};
use serde_json::json;

use super::HelperTrustHub;
use super::verify::VerifiedReport;

/// The system actor the attestation transitions are recorded under.
pub(super) const ACTOR: &str = "helper-trust";

pub(super) fn system() -> Actor {
    Actor::System(ACTOR.to_owned())
}

pub(super) fn audit(
    hub: &HelperTrustHub,
    actor: Actor,
    kind: &str,
    host: HostId,
    detail: serde_json::Value,
) -> AuditDraft {
    AuditDraft::new(
        actor,
        hub.clock.now(),
        kind.to_owned(),
        format!("host:{host}"),
        detail,
    )
}

fn was_blocked(previous: &StoredTrust) -> bool {
    matches!(
        previous.last.as_ref().map(|last| last.outcome),
        Some(LatestOutcome::Mismatch | LatestOutcome::SignedIncomplete | LatestOutcome::Regressed)
    )
}

impl HelperTrustHub {
    /// Stores a verified report, advances the high-water mark and sets the latest outcome to
    /// verified in one commit. `seen` is the high-water row the verifier read; the store
    /// answers a conflict if it moved.
    pub(super) async fn commit_accepted(
        &self,
        host: HostId,
        pin: &PinnedKey,
        verified: Box<VerifiedReport>,
        seen: Option<HighWater>,
        previous: &StoredTrust,
    ) -> Result<(), CommitError> {
        let parsed = verified.parsed();
        let identity = SafetyIdentity {
            fingerprint: pin.fingerprint,
            boot_id: parsed.boot_id.clone(),
            generation: parsed.generation,
            config_hash: parsed.config_hash,
            route_state_hash: parsed.route_state_hash,
        };
        let nonce_issued_at = verified.nonce_issued_at();
        let generation = identity.generation;
        let accepted = Accepted {
            host,
            identity,
            report: verified.into_bytes(),
            nonce_issued_at,
            accepted_at: self.clock.now(),
            seen_high_water: seen,
        };
        let mut tx =
            Transaction::new().write(StateWrite::HelperTrust(HelperTrustWrite::Accept(accepted)));
        if was_blocked(previous) {
            tx = tx.audit(audit(
                self,
                system(),
                "helper.attestation_restored",
                host,
                json!({
                    "fingerprint": pin.fingerprint.to_string(),
                    "generation": generation,
                }),
            ));
        }
        self.uow.commit(tx).await.map(|_| ())
    }

    /// Records a non-verified latest outcome, which revokes the stored verified state, with
    /// the audit rows of the transition.
    pub(super) async fn commit_outcome(
        &self,
        host: HostId,
        outcome: LatestOutcome,
        reason: UnverifiedReason,
        detail: &str,
        previous: &StoredTrust,
    ) -> Result<(), CommitError> {
        let before = previous.last.as_ref().map(|last| last.outcome);
        let record = OutcomeRecord {
            host,
            outcome,
            reason,
            detail: detail.to_owned(),
            at: self.clock.now(),
        };
        let mut tx = Transaction::new().write(StateWrite::HelperTrust(
            HelperTrustWrite::RecordOutcome(record),
        ));
        let info = json!({"reason": reason.token(), "detail": detail});
        if previous.view.verified.is_some() {
            tx = tx.audit(audit(
                self,
                system(),
                "helper.attestation_revoked",
                host,
                info.clone(),
            ));
        }
        if before != Some(outcome) {
            let kind = match outcome {
                LatestOutcome::Mismatch => Some("helper.key_mismatch"),
                LatestOutcome::Regressed => Some("helper.generation_regressed"),
                LatestOutcome::Verified | LatestOutcome::SignedIncomplete => None,
            };
            if let Some(kind) = kind {
                tx = tx.audit(audit(self, system(), kind, host, info));
            }
        }
        self.uow.commit(tx).await.map(|_| ())
    }
}
