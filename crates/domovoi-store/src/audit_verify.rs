//! `Store::verify_audit`, `Store::audit_head` and `Store::audit_anchor`: reading the chain and
//! reporting the legacy and chained histories separately.

use domovoi_core::audit_chain::{
    AuditAnchor, ChainStart, ChainStatus, ChainVerifier, Hash, LegacyDigest, LegacyStatus,
    VerifyReport, VerifyScope,
};
use domovoi_core::ports::StoreError;
use serde_json::Value;
use sqlx::SqliteConnection;

use crate::audit_chain::{
    BATCH, checkpoints, count_legacy, first_chained, load_anchor, newest_chained, newest_rows,
    rows_after,
};
use crate::db::Store;
use crate::error::db;

impl Store {
    /// Verifies the audit history. `Full` checks the legacy digest and every retained chained row;
    /// `Last(n)` checks the legacy row count only and the newest n chained rows.
    pub async fn verify_audit(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        let anchor = load_anchor(&mut conn)
            .await?
            .ok_or_else(|| StoreError::Backend("audit anchor is missing".to_owned()))?;
        match scope {
            VerifyScope::Full => verify_full(&mut conn, anchor).await,
            VerifyScope::Last(n) => verify_last(&mut conn, anchor, n.max(1)).await,
        }
    }

    /// The newest chained row's sequence number and hash, or `None` before the first chained row.
    pub async fn audit_head(&self) -> Result<Option<(u64, Hash)>, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        newest_chained(&mut conn).await
    }

    /// The anchor, `None` only if the store was not opened through `Store::open`.
    pub async fn audit_anchor(&self) -> Result<Option<AuditAnchor>, StoreError> {
        let mut conn = self.read.acquire().await.map_err(db)?;
        load_anchor(&mut conn).await
    }
}

async fn verify_full(
    conn: &mut SqliteConnection,
    anchor: AuditAnchor,
) -> Result<VerifyReport, StoreError> {
    let legacy = if legacy_pruned(conn).await? {
        LegacyStatus::NotVerifiable
    } else {
        verify_legacy(conn, &anchor).await?
    };
    let chained = verify_chained(conn, &anchor).await?;
    Ok(VerifyReport {
        legacy,
        chained,
        anchor,
    })
}

async fn verify_legacy(
    conn: &mut SqliteConnection,
    anchor: &AuditAnchor,
) -> Result<LegacyStatus, StoreError> {
    let mut digest = LegacyDigest::new();
    let mut after = 0;
    loop {
        let rows = rows_after(conn, after, Some(anchor.legacy_last_id), BATCH).await?;
        if rows.is_empty() {
            break;
        }
        for row in &rows {
            digest.update(row.seq, &row.event);
        }
        after = rows[rows.len() - 1].seq;
    }
    let (actual_count, actual_digest) = digest.finish();
    if actual_count == anchor.legacy_count && actual_digest == anchor.legacy_digest {
        Ok(LegacyStatus::Verified {
            count: actual_count,
        })
    } else {
        Ok(LegacyStatus::Mismatch {
            expected_count: anchor.legacy_count,
            actual_count,
            digest_matches: actual_digest == anchor.legacy_digest,
        })
    }
}

async fn verify_chained(
    conn: &mut SqliteConnection,
    anchor: &AuditAnchor,
) -> Result<ChainStatus, StoreError> {
    let Some(first) = first_chained(conn, anchor.legacy_last_id).await? else {
        return Ok(ChainStatus::Empty);
    };
    let start = if first.seq == anchor.legacy_last_id.saturating_add(1) {
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
    let mut after = anchor.legacy_last_id;
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
    let actual_count = count_legacy(conn, anchor.legacy_last_id).await?;
    let legacy = if actual_count == anchor.legacy_count {
        LegacyStatus::CountOnly {
            count: actual_count,
        }
    } else {
        LegacyStatus::Mismatch {
            expected_count: anchor.legacy_count,
            actual_count,
            digest_matches: false,
        }
    };

    let rows = newest_rows(conn, anchor.legacy_last_id, n).await?;
    let chained = if rows.is_empty() {
        ChainStatus::Empty
    } else {
        let start = if rows[0].seq == anchor.legacy_last_id.saturating_add(1) {
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
    Ok(VerifyReport {
        legacy,
        chained,
        anchor,
    })
}

/// A `Trusted` start when the newest checkpoint names this row and links to it.
async fn checkpoint_start(
    conn: &mut SqliteConnection,
    first_seq: u64,
    first_prev: Option<&Hash>,
) -> Result<Option<ChainStart>, StoreError> {
    let Some(prev) = first_prev else {
        return Ok(None);
    };
    let expected = hex::encode(prev);
    let found = checkpoints(conn).await?.into_iter().any(|detail| {
        detail.get("first_retained_seq").and_then(Value::as_u64) == Some(first_seq)
            && detail
                .get("first_retained_prev_hash")
                .and_then(Value::as_str)
                == Some(expected.as_str())
    });
    Ok(found.then_some(ChainStart::Trusted))
}

async fn legacy_pruned(conn: &mut SqliteConnection) -> Result<bool, StoreError> {
    Ok(checkpoints(conn).await?.into_iter().any(|detail| {
        detail
            .get("legacy_pruned")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }))
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
