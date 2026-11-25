//! Applying the helper trust writes on one SQLite connection.
//!
//! `Accept` re-evaluates the high-water rules inside the transaction, so two concurrent
//! verifications of one host cannot both advance the mark.

use domovoi_core::HostId;
use domovoi_core::attest::LatestOutcome;
use domovoi_core::ports::{
    Accepted, AggregateRef, CommitError, Expectation, HelperTrustWrite, HighWater, Observed,
    OutcomeRecord, PinWrite,
};
use sqlx::SqliteConnection;

use super::read::high_water_row;
use crate::codec::{int, ms};
use crate::uow::{backend, map_store};

/// Applies one helper trust write.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &HelperTrustWrite,
) -> Result<(), CommitError> {
    match write {
        HelperTrustWrite::Accept(accepted) => accept(conn, accepted).await,
        HelperTrustWrite::RecordOutcome(record) => record_outcome(conn, record).await,
        HelperTrustWrite::Pin(pin) => pin_key(conn, pin).await,
        HelperTrustWrite::Unpin { host } => delete_rows(conn, *host, &UNPIN_TABLES).await,
        HelperTrustWrite::RemoveHost { host } => delete_rows(conn, *host, &ALL_TABLES).await,
    }
}

/// The tables an unpin clears: the pin and the latest outcome; high-water rows stay.
const UNPIN_TABLES: [&str; 2] = ["helper_keys", "helper_attestation"];
/// The tables a removed host loses rows from.
const ALL_TABLES: [&str; 3] = ["helper_keys", "helper_attestation", "helper_high_water"];

fn conflict(host: HostId, expected: Option<&HighWater>, found: Option<&HighWater>) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::helper_trust(host),
        expected: Expectation::Version(expected.map_or(0, |row| row.generation)),
        found: found.map_or(Observed::Missing, |row| Observed::Version(row.generation)),
    }
}

async fn accept(conn: &mut SqliteConnection, accepted: &Accepted) -> Result<(), CommitError> {
    let host = accepted.host;
    let identity = &accepted.identity;
    let stored = high_water_row(conn, host, &identity.fingerprint.to_string())
        .await
        .map_err(map_store)?;
    if stored != accepted.seen_high_water {
        return Err(conflict(
            host,
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
            return Err(conflict(host, stored.as_ref(), stored.as_ref()));
        }
    }
    sqlx::query(
        "INSERT INTO helper_high_water \
         (host_id, fingerprint, generation, config_hash, route_state_hash, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (host_id, fingerprint) DO UPDATE SET generation = ?3, config_hash = ?4, \
         route_state_hash = ?5, updated_at = ?6",
    )
    .bind(host.to_string())
    .bind(identity.fingerprint.to_string())
    .bind(int(identity.generation))
    .bind(identity.config_hash.to_string())
    .bind(identity.route_state_hash.to_string())
    .bind(ms(accepted.accepted_at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    sqlx::query(
        "INSERT INTO helper_attestation \
         (host_id, latest_outcome, latest_reason, latest_detail, latest_at, fingerprint, report, \
          generation, boot_id, config_hash, route_state_hash, nonce_issued_at, accepted_at) \
         VALUES (?1, 'verified', NULL, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?2) \
         ON CONFLICT (host_id) DO UPDATE SET latest_outcome = 'verified', latest_reason = NULL, \
         latest_detail = NULL, latest_at = ?2, fingerprint = ?3, report = ?4, generation = ?5, \
         boot_id = ?6, config_hash = ?7, route_state_hash = ?8, nonce_issued_at = ?9, \
         accepted_at = ?2, version = version + 1",
    )
    .bind(host.to_string())
    .bind(ms(accepted.accepted_at))
    .bind(identity.fingerprint.to_string())
    .bind(accepted.report.as_slice())
    .bind(int(identity.generation))
    .bind(identity.boot_id.as_str())
    .bind(identity.config_hash.to_string())
    .bind(identity.route_state_hash.to_string())
    .bind(ms(accepted.nonce_issued_at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}

async fn record_outcome(
    conn: &mut SqliteConnection,
    record: &OutcomeRecord,
) -> Result<(), CommitError> {
    if record.outcome == LatestOutcome::Verified {
        return Err(CommitError::Constraint(
            "a verified outcome is stored by an accepted report only".to_owned(),
        ));
    }
    sqlx::query(
        "INSERT INTO helper_attestation (host_id, latest_outcome, latest_reason, latest_detail, \
         latest_at) VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (host_id) DO UPDATE SET latest_outcome = ?2, latest_reason = ?3, \
         latest_detail = ?4, latest_at = ?5, fingerprint = NULL, report = NULL, generation = NULL, \
         boot_id = NULL, config_hash = NULL, route_state_hash = NULL, nonce_issued_at = NULL, \
         accepted_at = NULL, version = version + 1",
    )
    .bind(record.host.to_string())
    .bind(record.outcome.as_str())
    .bind(record.reason.token())
    .bind(&record.detail)
    .bind(ms(record.at))
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    Ok(())
}

async fn pin_key(conn: &mut SqliteConnection, pin: &PinWrite) -> Result<(), CommitError> {
    let host = pin.host.to_string();
    sqlx::query(
        "INSERT INTO helper_keys (host_id, public_key, fingerprint, pinned_at, pinned_by) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (host_id) DO UPDATE SET public_key = ?2, fingerprint = ?3, pinned_at = ?4, \
         pinned_by = ?5",
    )
    .bind(&host)
    .bind(pin.public_key.as_slice())
    .bind(pin.fingerprint.to_string())
    .bind(ms(pin.pinned_at))
    .bind(&pin.pinned_by)
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    sqlx::query("DELETE FROM helper_high_water WHERE host_id = ? AND fingerprint = ?")
        .bind(&host)
        .bind(pin.fingerprint.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    sqlx::query("DELETE FROM helper_attestation WHERE host_id = ?")
        .bind(&host)
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn delete_rows(
    conn: &mut SqliteConnection,
    host: HostId,
    tables: &[&'static str],
) -> Result<(), CommitError> {
    for table in tables {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE host_id = ?"
        )))
        .bind(host.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    }
    Ok(())
}
