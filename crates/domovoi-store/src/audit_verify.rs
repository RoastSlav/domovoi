//! `Store::verify_audit`, `Store::audit_head` and `Store::audit_anchor`: reading and verifying the chain.

use domovoi_core::audit_chain::{
    AuditAnchor, ChainStart, ChainStatus, ChainVerifier, Hash, VerifyReport, VerifyScope,
};
use domovoi_core::ports::StoreError;
use serde_json::Value;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, SqliteConnection};

use crate::audit_chain::{
    BATCH, checked_anchor, checkpoints, first_chained, newest_chained, newest_rows, rows_after,
};
use crate::backup::BackupError;
use crate::db::Store;
use crate::error::db;

impl Store {
    /// Verifies the audit history. `Full` checks every retained row; `Last(n)` checks the newest
    /// n rows.
    pub async fn verify_audit(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        let anchor = checked_anchor(&mut conn)
            .await?
            .ok_or_else(|| StoreError::Backend("audit anchor is missing".to_owned()))?;
        match scope {
            VerifyScope::Full => verify_full(&mut conn, anchor).await,
            VerifyScope::Last(n) => verify_last(&mut conn, anchor, n.max(1)).await,
        }
    }

    /// Edits the detail of audit row `seq` behind the append-only triggers, so a test can
    /// hand the verifier a history that was tampered with.
    #[doc(hidden)]
    pub async fn edit_audit_row_for_tests(&self, seq: u64) -> Result<(), StoreError> {
        let pool = self.pool_for_tests();
        sqlx::query("DROP TRIGGER audit_log_no_update")
            .execute(pool)
            .await
            .map_err(db)?;
        sqlx::query("UPDATE audit_log SET detail = '{\"edited\":true}' WHERE seq = ?")
            .bind(i64::try_from(seq).unwrap_or(i64::MAX))
            .execute(pool)
            .await
            .map_err(db)?;
        sqlx::query(
            "CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN              SELECT RAISE(ABORT, 'audit_log is append-only'); END",
        )
        .execute(pool)
        .await
        .map_err(db)?;
        Ok(())
    }

    /// The newest row's sequence number and hash, or `None` before the first row.
    pub async fn audit_head(&self) -> Result<Option<(u64, Hash)>, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        newest_chained(&mut conn).await
    }

    /// The anchor, `None` only if the store was not opened through `Store::open`.
    pub async fn audit_anchor(&self) -> Result<Option<AuditAnchor>, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        checked_anchor(&mut conn).await
    }
}

/// Verifies the whole audit chain of a backup file, opened read-only and never changed.
///
/// A backup with no audit anchor and no audit rows has nothing to check and reads as
/// [`ChainStatus::Empty`]; rows without an anchor are an error, as in a live database.
pub async fn verify_backup_audit(path: &std::path::Path) -> Result<ChainStatus, BackupError> {
    let unreadable = |err: &dyn std::fmt::Display| BackupError::Unreadable(err.to_string());
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await
        .map_err(|err| unreadable(&err))?;
    let outcome = async {
        match checked_anchor(&mut conn).await? {
            Some(anchor) => Ok(verify_full(&mut conn, anchor).await?.chained),
            None => match first_chained(&mut conn).await? {
                None => Ok(ChainStatus::Empty),
                Some(_) => Err(StoreError::Backend("audit anchor is missing".to_owned())),
            },
        }
    }
    .await;
    let _ = conn.close().await;
    outcome.map_err(|err: StoreError| unreadable(&err))
}

async fn verify_full(
    conn: &mut SqliteConnection,
    anchor: AuditAnchor,
) -> Result<VerifyReport, StoreError> {
    let chained = verify_chained(conn, &anchor).await?;
    Ok(VerifyReport { chained, anchor })
}

async fn verify_chained(
    conn: &mut SqliteConnection,
    anchor: &AuditAnchor,
) -> Result<ChainStatus, StoreError> {
    let Some(first) = first_chained(conn).await? else {
        return Ok(ChainStatus::Empty);
    };
    let start = if first.seq == 1 {
        ChainStart::Anchor {
            anchor_hash: anchor.hash(),
            first_seq: first.seq,
        }
    } else {
        match checkpoint_start(conn, first.seq, first.prev_hash.as_ref()).await? {
            Some(start) => start,
            None => {
                return Ok(ChainStatus::Broken {
                    checked: 0,
                    first_broken_seq: first.seq,
                    reason: domovoi_core::audit_chain::BreakReason::CheckpointMismatch,
                });
            }
        }
    };

    let mut verifier = ChainVerifier::new(start);
    let mut after = 0;
    loop {
        let rows = rows_after(conn, after, None, BATCH).await?;
        if rows.is_empty() {
            break;
        }
        for row in &rows {
            if let Err(brk) = verifier.push(row) {
                return Ok(ChainStatus::Broken {
                    checked: verifier.checked(),
                    first_broken_seq: brk.seq,
                    reason: brk.reason,
                });
            }
        }
        after = rows[rows.len() - 1].seq;
    }
    Ok(status_of(&verifier))
}

async fn verify_last(
    conn: &mut SqliteConnection,
    anchor: AuditAnchor,
    n: u64,
) -> Result<VerifyReport, StoreError> {
    let rows = newest_rows(conn, n).await?;
    let chained = if rows.is_empty() {
        ChainStatus::Empty
    } else {
        let start = if rows[0].seq == 1 {
            ChainStart::Anchor {
                anchor_hash: anchor.hash(),
                first_seq: rows[0].seq,
            }
        } else {
            ChainStart::Trusted
        };
        let mut verifier = ChainVerifier::new(start);
        let mut broken = None;
        for row in &rows {
            if let Err(brk) = verifier.push(row) {
                broken = Some(brk);
                break;
            }
        }
        match broken {
            Some(brk) => ChainStatus::Broken {
                checked: verifier.checked(),
                first_broken_seq: brk.seq,
                reason: brk.reason,
            },
            None => status_of(&verifier),
        }
    };
    Ok(VerifyReport { chained, anchor })
}

/// A `Trusted` start when the newest checkpoint for this retained row links to it.
async fn checkpoint_start(
    conn: &mut SqliteConnection,
    first_seq: u64,
    first_prev: Option<&Hash>,
) -> Result<Option<ChainStart>, StoreError> {
    let Some(prev) = first_prev else {
        return Ok(None);
    };
    let expected = hex::encode(prev);
    // `checkpoints` is newest first, so the first match is the checkpoint that wins.
    let newest = checkpoints(conn).await?.into_iter().find(|(_, detail)| {
        detail.get("first_retained_seq").and_then(Value::as_u64) == Some(first_seq)
    });
    let Some((_, detail)) = newest else {
        return Ok(None);
    };
    let matches = detail
        .get("first_retained_prev_hash")
        .and_then(Value::as_str)
        == Some(expected.as_str());
    Ok(matches.then_some(ChainStart::Trusted))
}

fn status_of(verifier: &ChainVerifier) -> ChainStatus {
    match verifier.head() {
        Some((head_seq, head_hash)) => ChainStatus::Intact {
            checked: verifier.checked(),
            head_seq,
            head_hash,
        },
        None => ChainStatus::Empty,
    }
}
