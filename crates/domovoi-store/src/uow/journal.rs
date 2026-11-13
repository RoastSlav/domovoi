//! Appending journal rows inside the transaction.

use domovoi_core::ports::{CommitError, JournalDraft};
use sqlx::SqliteConnection;

use crate::codec::ms;

use super::backend;

/// Appends a journal row and returns its sequence number.
pub(super) async fn insert(
    conn: &mut SqliteConnection,
    draft: &JournalDraft,
    legacy_event_seq: Option<u64>,
) -> Result<u64, CommitError> {
    const SQL: &str = "INSERT INTO journal \
        (aggregate_type, aggregate_id, kind, payload_v, payload, actor, occurred_at, \
         legacy_event_seq) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING seq";
    let payload = serde_json::to_string(&draft.payload)
        .map_err(|e| CommitError::Backend(format!("cannot encode journal payload: {e}")))?;
    let legacy = legacy_event_seq.map(|seq| i64::try_from(seq).unwrap_or(i64::MAX));
    let (seq,): (i64,) = sqlx::query_as(SQL)
        .bind(draft.aggregate.kind.as_str())
        .bind(&draft.aggregate.id)
        .bind(draft.kind.as_str())
        .bind(i64::from(draft.payload_v))
        .bind(payload)
        .bind(&draft.actor)
        .bind(ms(draft.occurred_at))
        .bind(legacy)
        .fetch_one(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(crate::codec::uint(seq))
}
