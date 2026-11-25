//! The in-memory twin of the helper trust tables: the same guards as the SQLite applier (the
//! high-water conflict check, the pin resetting its high-water row, unpin keeping it).

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::{
    LatestOutcome, PinnedKey, Report, TrustView, VerifiedState, parse_report,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{
    AggregateRef, CommitError, Expectation, HelperTrustRepo, HelperTrustWrite, HighWater,
    LastOutcome, Observed, StoreError, StoredTrust,
};

use super::{InMemoryStore, State};

/// One host's latest outcome row.
#[derive(Clone)]
pub(super) struct AttestationRow {
    last: LastOutcome,
    verified: Option<VerifiedState>,
}

/// The pins, latest outcomes and high-water marks.
#[derive(Default, Clone)]
pub(super) struct HelperState {
    pins: BTreeMap<HostId, PinnedKey>,
    attestations: BTreeMap<HostId, AttestationRow>,
    high_water: BTreeMap<(HostId, KeyFingerprint), HighWater>,
}

fn conflict(host: HostId, expected: Option<&HighWater>, found: Option<&HighWater>) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::helper_trust(host),
        expected: Expectation::Version(expected.map_or(0, |row| row.generation)),
        found: found.map_or(Observed::Missing, |row| Observed::Version(row.generation)),
    }
}

/// Stores one helper trust write.
pub(super) fn apply(state: &mut State, write: &HelperTrustWrite) -> Result<(), CommitError> {
    let helper = &mut state.helper;
    match write {
        HelperTrustWrite::Accept(accepted) => {
            let identity = &accepted.identity;
            let key = (accepted.host, identity.fingerprint);
            let stored = helper.high_water.get(&key).cloned();
            if stored != accepted.seen_high_water {
                return Err(conflict(
                    accepted.host,
                    accepted.seen_high_water.as_ref(),
                    stored.as_ref(),
                ));
            }
            if let Some(row) = &stored {
                let regressed = identity.generation < row.generation;
                let repeated = identity.generation == row.generation
                    && (identity.config_hash != row.config_hash
                        || identity.route_state_hash != row.route_state_hash);
                if regressed || repeated {
                    return Err(conflict(accepted.host, stored.as_ref(), stored.as_ref()));
                }
            }
            helper.high_water.insert(
                key,
                HighWater {
                    generation: identity.generation,
                    config_hash: identity.config_hash,
                    route_state_hash: identity.route_state_hash,
                },
            );
            let Ok(Report::Complete(parsed)) = parse_report(&accepted.report) else {
                return Err(CommitError::Constraint(
                    "the accepted report is not a complete report".to_owned(),
                ));
            };
            helper.attestations.insert(
                accepted.host,
                AttestationRow {
                    last: LastOutcome {
                        outcome: LatestOutcome::Verified,
                        reason: None,
                        detail: None,
                        at: accepted.accepted_at,
                    },
                    verified: Some(VerifiedState::from_report(
                        &parsed,
                        accepted.report.clone(),
                        identity.fingerprint,
                        accepted.nonce_issued_at,
                        accepted.accepted_at,
                    )),
                },
            );
        }
        HelperTrustWrite::RecordOutcome(record) => {
            if record.outcome == LatestOutcome::Verified {
                return Err(CommitError::Constraint(
                    "a verified outcome is stored by an accepted report only".to_owned(),
                ));
            }
            helper.attestations.insert(
                record.host,
                AttestationRow {
                    last: LastOutcome {
                        outcome: record.outcome,
                        reason: Some(record.reason),
                        detail: Some(record.detail.clone()),
                        at: record.at,
                    },
                    verified: None,
                },
            );
        }
        HelperTrustWrite::Pin(pin) => {
            helper.pins.insert(
                pin.host,
                PinnedKey::new(pin.public_key, pin.pinned_at, pin.pinned_by.clone()),
            );
            helper.high_water.remove(&(pin.host, pin.fingerprint));
            helper.attestations.remove(&pin.host);
        }
        HelperTrustWrite::Unpin { host } => {
            helper.pins.remove(host);
            helper.attestations.remove(host);
        }
        HelperTrustWrite::RemoveHost { host } => {
            helper.pins.remove(host);
            helper.attestations.remove(host);
            helper.high_water.retain(|(owner, _), _| owner != host);
        }
    }
    Ok(())
}

#[async_trait]
impl HelperTrustRepo for InMemoryStore {
    async fn stored(&self, host: HostId) -> Result<StoredTrust, StoreError> {
        let state = self.begin()?;
        let row = state.helper.attestations.get(&host);
        Ok(StoredTrust {
            view: TrustView {
                pin: state.helper.pins.get(&host).cloned(),
                received_key: None,
                latest: row.map(|row| row.last.outcome),
                verified: row.and_then(|row| row.verified.clone()),
            },
            last: row.map(|row| row.last.clone()),
        })
    }

    async fn high_water(
        &self,
        host: HostId,
        fingerprint: &KeyFingerprint,
    ) -> Result<Option<HighWater>, StoreError> {
        let state = self.begin()?;
        Ok(state.helper.high_water.get(&(host, *fingerprint)).cloned())
    }
}
