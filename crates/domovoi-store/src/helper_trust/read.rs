//! Reading the helper trust rows: the pin, the latest outcome with its verified state, and the
//! high-water mark.

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::{
    Hash32, LatestOutcome, PinnedKey, Report, TrustView, UnverifiedReason, VerifiedState,
    parse_report,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{HelperTrustRepo, HighWater, LastOutcome, StoreError, StoredTrust};
use sqlx::SqliteConnection;

use crate::codec::{from_ms, hash32, parse, uint};
use crate::db::Store;
use crate::error::{corrupt, db};

type HighWaterRow = (i64, String, String);

fn high_water_of(row: HighWaterRow) -> Result<HighWater, StoreError> {
    let (generation, config_hash, route_state_hash) = row;
    Ok(HighWater {
        generation: uint(generation),
        config_hash: parse::<Hash32>(&config_hash, "helper high-water config hash")?,
        route_state_hash: parse::<Hash32>(&route_state_hash, "helper high-water route hash")?,
    })
}

/// The high-water row of `host` under the key with `fingerprint` (64 hex), on `conn`.
pub(super) async fn high_water_row(
    conn: &mut SqliteConnection,
    host: HostId,
    fingerprint: &str,
) -> Result<Option<HighWater>, StoreError> {
    let row: Option<HighWaterRow> = sqlx::query_as(
        "SELECT generation, config_hash, route_state_hash FROM helper_high_water \
         WHERE host_id = ? AND fingerprint = ?",
    )
    .bind(host.to_string())
    .bind(fingerprint)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db)?;
    row.map(high_water_of).transpose()
}

type PinRow = (Vec<u8>, i64, String);

#[derive(sqlx::FromRow)]
struct AttestationRow {
    latest_outcome: String,
    latest_reason: Option<String>,
    latest_detail: Option<String>,
    latest_at: i64,
    fingerprint: Option<String>,
    report: Option<Vec<u8>>,
    nonce_issued_at: Option<i64>,
    accepted_at: Option<i64>,
}

fn pinned(row: PinRow) -> Result<PinnedKey, StoreError> {
    let (public_key, pinned_at, pinned_by) = row;
    Ok(PinnedKey::new(
        hash32(&public_key, "helper public key")?,
        from_ms(pinned_at)?,
        pinned_by,
    ))
}

impl AttestationRow {
    fn last_and_verified(self) -> Result<(LastOutcome, Option<VerifiedState>), StoreError> {
        let outcome = LatestOutcome::parse(&self.latest_outcome)
            .ok_or_else(|| corrupt("helper latest outcome", &self.latest_outcome))?;
        let reason = self
            .latest_reason
            .as_deref()
            .map(UnverifiedReason::parse)
            .transpose()
            .map_err(|e| corrupt("helper latest reason", e))?;
        let last = LastOutcome {
            outcome,
            reason,
            detail: self.latest_detail,
            at: from_ms(self.latest_at)?,
        };
        let verified = match (outcome, self.report, self.fingerprint) {
            (LatestOutcome::Verified, Some(report), Some(fingerprint)) => {
                let issued = self
                    .nonce_issued_at
                    .ok_or_else(|| corrupt("helper attestation", "no nonce issue time"))?;
                let accepted = self
                    .accepted_at
                    .ok_or_else(|| corrupt("helper attestation", "no accept time"))?;
                let Report::Complete(parsed) =
                    parse_report(&report).map_err(|e| corrupt("helper report", e))?
                else {
                    return Err(corrupt("helper report", "stored report is incomplete"));
                };
                Some(VerifiedState::from_report(
                    &parsed,
                    report,
                    parse::<KeyFingerprint>(&fingerprint, "helper fingerprint")?,
                    from_ms(issued)?,
                    from_ms(accepted)?,
                ))
            }
            _ => None,
        };
        Ok((last, verified))
    }
}

#[async_trait]
impl HelperTrustRepo for Store {
    async fn stored(&self, host: HostId) -> Result<StoredTrust, StoreError> {
        let pin: Option<PinRow> = sqlx::query_as(
            "SELECT public_key, pinned_at, pinned_by FROM helper_keys WHERE host_id = ?",
        )
        .bind(host.to_string())
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        let attestation: Option<AttestationRow> = sqlx::query_as(
            "SELECT latest_outcome, latest_reason, latest_detail, latest_at, fingerprint, report, \
             nonce_issued_at, accepted_at FROM helper_attestation WHERE host_id = ?",
        )
        .bind(host.to_string())
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        let (last, verified) = match attestation {
            Some(row) => {
                let (last, verified) = row.last_and_verified()?;
                (Some(last), verified)
            }
            None => (None, None),
        };
        Ok(StoredTrust {
            view: TrustView {
                pin: pin.map(pinned).transpose()?,
                received_key: None,
                latest: last.as_ref().map(|last| last.outcome),
                verified,
            },
            last,
        })
    }

    async fn high_water(
        &self,
        host: HostId,
        fingerprint: &KeyFingerprint,
    ) -> Result<Option<HighWater>, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        high_water_row(&mut conn, host, &fingerprint.to_string()).await
    }
}
