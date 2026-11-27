//! Inserting chained audit rows and reading them back for verification.
//!
//! [`insert_audit`] is the only code that inserts into `audit_log`. It runs inside the caller's
//! `BEGIN IMMEDIATE` transaction, so a failed commit rolls the chain position back with it.

use domovoi_core::audit_chain::{AuditAnchor, CHECKPOINT_KIND, ChainRow, Hash, entry_hash};
use domovoi_core::ports::{AuditEvent, StoreError};
use jiff::Timestamp;
use serde_json::Value;
use sqlx::{SqliteConnection, SqlitePool};
use uuid::Uuid;

use crate::codec::{from_json, from_ms, hash32, int, json, ms, uint};
use crate::error::{StoreOpenError, db};

/// Rows read per statement while streaming history.
pub(crate) const BATCH: u32 = 1000;

#[derive(sqlx::FromRow)]
pub(crate) struct AuditChainRow {
    pub(crate) seq: i64,
    at: i64,
    actor: String,
    kind: String,
    subject: String,
    detail: String,
    prev_hash: Option<Vec<u8>>,
    hash: Option<Vec<u8>>,
}

impl AuditChainRow {
    pub(crate) fn into_chain_row(self) -> Result<ChainRow, StoreError> {
        let prev_hash = self
            .prev_hash
            .as_deref()
            .map(|bytes| hash32(bytes, "audit prev_hash"))
            .transpose()?;
        let hash = self
            .hash
            .as_deref()
            .map(|bytes| hash32(bytes, "audit hash"))
            .transpose()?;
        let event = AuditEvent {
            at: from_ms(self.at)?,
            actor: self.actor,
            kind: self.kind,
            subject: self.subject,
            detail: from_json(&self.detail, "audit detail")?,
        };
        Ok(ChainRow {
            seq: uint(self.seq),
            prev_hash,
            hash,
            event,
        })
    }
}

/// Appends an audit row, links it to the previous chained row (or the anchor) and returns its
/// sequence number. The caller owns the `BEGIN IMMEDIATE` transaction.
pub(crate) async fn insert_audit(
    conn: &mut SqliteConnection,
    event: &AuditEvent,
) -> Result<u64, StoreError> {
    const SQL: &str = "INSERT INTO audit_log \
        (seq, at, actor, kind, subject, detail, prev_hash, hash) \
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)";
    let prev = match newest_chained(conn).await? {
        Some((_, hash)) => hash,
        None => checked_anchor(conn)
            .await?
            .ok_or_else(|| StoreError::Backend("audit anchor is missing".to_owned()))?
            .hash(),
    };
    let seq = next_seq(conn).await?;

    // Hash the event exactly as it will round-trip through the stored columns.
    let stored = AuditEvent {
        at: from_ms(ms(event.at))?,
        actor: event.actor.clone(),
        kind: event.kind.clone(),
        subject: event.subject.clone(),
        detail: from_json(&json(&event.detail)?, "audit detail")?,
    };
    let hash = entry_hash(seq, &prev, &stored);

    sqlx::query(SQL)
        .bind(int(seq))
        .bind(ms(stored.at))
        .bind(&stored.actor)
        .bind(&stored.kind)
        .bind(&stored.subject)
        .bind(json(&stored.detail)?)
        .bind(prev.as_slice())
        .bind(hash.as_slice())
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    log_pointer(seq, &stored);
    Ok(seq)
}

/// Audit kinds that happen too often for a line each at the default level.
const QUIET_KINDS: [&str; 2] = [CHECKPOINT_KIND, "ack_rule.matched"];

/// One line that points at the audit row: its number, kind, actor and subject. The audit
/// log is the record; this line lets a person reading the log find the row. The detail is
/// left out, since it can hold values that must not reach the log.
fn log_pointer(seq: u64, event: &AuditEvent) {
    if QUIET_KINDS.contains(&event.kind.as_str()) {
        tracing::debug!(seq, kind = %event.kind, actor = %event.actor, subject = %event.subject, "audit row recorded");
    } else {
        tracing::info!(seq, kind = %event.kind, actor = %event.actor, subject = %event.subject, "audit row recorded");
    }
}

/// Writes the single anchor row if it does not exist yet.
///
/// Called once in the open sequence, after migrations, in its own `BEGIN IMMEDIATE` transaction.
pub(crate) async fn ensure_anchor(pool: &SqlitePool, now: Timestamp) -> Result<(), StoreOpenError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(open_db)?;
    if load_anchor(&mut tx).await.map_err(open_store)?.is_some() {
        tx.commit().await.map_err(open_db)?;
        return Ok(());
    }

    let chained: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(&mut *tx)
        .await
        .map_err(open_db)?;
    if chained > 0 {
        return Err(StoreOpenError::Database(
            "audit chain exists without an anchor".to_owned(),
        ));
    }

    let anchor = AuditAnchor {
        install_id: Uuid::new_v4().to_string(),
    };

    sqlx::query(
        "INSERT INTO audit_anchor (id, install_id, anchor_hash, created_at) VALUES (1, ?, ?, ?)",
    )
    .bind(&anchor.install_id)
    .bind(anchor.hash().as_slice())
    .bind(ms(now))
    .execute(&mut *tx)
    .await
    .map_err(open_db)?;
    tx.commit().await.map_err(open_db)?;
    Ok(())
}

/// The anchor row with the hash the database persists for it.
pub(crate) struct StoredAnchor {
    /// The anchor's fields.
    pub(crate) anchor: AuditAnchor,
    /// The hash stored beside them; it must equal [`AuditAnchor::hash`].
    pub(crate) anchor_hash: Hash,
}

/// The anchor row, `None` only if the store was not opened through `Store::open`.
pub(crate) async fn load_anchor(
    conn: &mut SqliteConnection,
) -> Result<Option<StoredAnchor>, StoreError> {
    #[derive(sqlx::FromRow)]
    struct AnchorRow {
        install_id: String,
        anchor_hash: Vec<u8>,
    }
    let row: Option<AnchorRow> =
        sqlx::query_as("SELECT install_id, anchor_hash FROM audit_anchor WHERE id = 1")
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
    row.map(|row| {
        Ok(StoredAnchor {
            anchor: AuditAnchor {
                install_id: row.install_id,
            },
            anchor_hash: hash32(&row.anchor_hash, "audit anchor hash")?,
        })
    })
    .transpose()
}

/// The anchor after checking the persisted hash against its contents.
///
/// A mismatch means the anchor was edited; verification and appends must not accept it.
pub(crate) async fn checked_anchor(
    conn: &mut SqliteConnection,
) -> Result<Option<AuditAnchor>, StoreError> {
    let Some(stored) = load_anchor(conn).await? else {
        return Ok(None);
    };
    if stored.anchor_hash != stored.anchor.hash() {
        return Err(StoreError::Backend(
            "audit anchor hash does not match its contents".to_owned(),
        ));
    }
    Ok(Some(stored.anchor))
}

/// The newest row that has a hash, with its sequence number.
pub(crate) async fn newest_chained(
    conn: &mut SqliteConnection,
) -> Result<Option<(u64, Hash)>, StoreError> {
    let row: Option<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT seq, hash FROM audit_log WHERE hash IS NOT NULL ORDER BY seq DESC LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(db)?;
    row.map(|(seq, hash)| Ok((uint(seq), hash32(&hash, "audit hash")?)))
        .transpose()
}

/// Rows with `after_seq < seq <= max_seq`, oldest first, at most `limit`.
pub(crate) async fn rows_after(
    conn: &mut SqliteConnection,
    after_seq: u64,
    max_seq: Option<u64>,
    limit: u32,
) -> Result<Vec<ChainRow>, StoreError> {
    let rows: Vec<AuditChainRow> = sqlx::query_as(
        "SELECT seq, at, actor, kind, subject, detail, prev_hash, hash FROM audit_log \
         WHERE seq > ?1 AND (?2 IS NULL OR seq <= ?2) ORDER BY seq ASC LIMIT ?3",
    )
    .bind(int(after_seq))
    .bind(max_seq.map(int))
    .bind(i64::from(limit))
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    rows.into_iter()
        .map(AuditChainRow::into_chain_row)
        .collect()
}

/// The lowest-sequence row, if any.
pub(crate) async fn first_chained(
    conn: &mut SqliteConnection,
) -> Result<Option<ChainRow>, StoreError> {
    let row: Option<AuditChainRow> = sqlx::query_as(
        "SELECT seq, at, actor, kind, subject, detail, prev_hash, hash FROM audit_log \
         ORDER BY seq ASC LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(db)?;
    row.map(AuditChainRow::into_chain_row).transpose()
}

/// The newest `limit` rows, oldest first.
pub(crate) async fn newest_rows(
    conn: &mut SqliteConnection,
    limit: u64,
) -> Result<Vec<ChainRow>, StoreError> {
    let rows: Vec<AuditChainRow> = sqlx::query_as(
        "SELECT seq, at, actor, kind, subject, detail, prev_hash, hash FROM audit_log \
         ORDER BY seq DESC LIMIT ?",
    )
    .bind(int(limit))
    .fetch_all(&mut *conn)
    .await
    .map_err(db)?;
    let mut rows = rows
        .into_iter()
        .map(AuditChainRow::into_chain_row)
        .collect::<Result<Vec<_>, _>>()?;
    rows.reverse();
    Ok(rows)
}

/// Every checkpoint row's sequence number and detail, newest first.
pub(crate) async fn checkpoints(
    conn: &mut SqliteConnection,
) -> Result<Vec<(u64, Value)>, StoreError> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT seq, detail FROM audit_log WHERE kind = ? ORDER BY seq DESC")
            .bind(CHECKPOINT_KIND)
            .fetch_all(&mut *conn)
            .await
            .map_err(db)?;
    rows.into_iter()
        .map(|(seq, detail)| Ok((uint(seq), from_json(&detail, "audit checkpoint detail")?)))
        .collect()
}

async fn next_seq(conn: &mut SqliteConnection) -> Result<u64, StoreError> {
    let auto: Option<i64> =
        sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'audit_log'")
            .fetch_optional(&mut *conn)
            .await
            .map_err(db)?;
    let (max,): (Option<i64>,) = sqlx::query_as("SELECT MAX(seq) FROM audit_log")
        .fetch_one(&mut *conn)
        .await
        .map_err(db)?;
    Ok(u64::max(uint(auto.unwrap_or(0)), uint(max.unwrap_or(0))) + 1)
}

fn open_db(err: sqlx::Error) -> StoreOpenError {
    StoreOpenError::Database(err.to_string())
}

fn open_store(err: StoreError) -> StoreOpenError {
    StoreOpenError::Database(err.to_string())
}
