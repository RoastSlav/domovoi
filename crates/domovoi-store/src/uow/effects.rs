//! Creating and completing effects inside the transaction.

use domovoi_core::ports::{CommitError, EffectCompletion, EffectDraft};
use sqlx::SqliteConnection;

use crate::codec::ms;

use super::backend;

/// Inserts a new pending effect.
pub(super) async fn insert(
    conn: &mut SqliteConnection,
    draft: &EffectDraft,
) -> Result<(), CommitError> {
    const SQL: &str = "INSERT INTO effects \
        (id, kind, aggregate_type, aggregate_id, payload, status, not_before, attempt, \
         max_attempts, dedup_key, claim_token, claim_generation, lease_until, runner_id, \
         last_error, created_at, completed_at) \
        VALUES (?, ?, ?, ?, ?, 'pending', ?, 0, ?, ?, NULL, 0, NULL, NULL, NULL, ?, NULL)";
    let payload = serde_json::to_string(&draft.payload)
        .map_err(|e| CommitError::Backend(format!("cannot encode effect payload: {e}")))?;
    if let Some(key) = &draft.dedup_key {
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT 1 FROM effects WHERE dedup_key = ? AND status IN ('pending', 'claimed')",
        )
        .bind(key)
        .fetch_optional(&mut *conn)
        .await
        .map_err(backend)?;
        if existing.is_some() {
            return Err(CommitError::Constraint("duplicate effect".to_owned()));
        }
    }
    sqlx::query(SQL)
        .bind(draft.id.to_string())
        .bind(draft.kind.as_str())
        .bind(draft.aggregate.kind.as_str())
        .bind(&draft.aggregate.id)
        .bind(payload)
        .bind(ms(draft.not_before))
        .bind(i64::from(draft.max_attempts))
        .bind(draft.dedup_key.as_deref())
        .bind(ms(draft.created_at))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

/// Marks an effect done, fenced by its current claim token or by its pending state.
pub(super) async fn complete(
    conn: &mut SqliteConnection,
    completion: &EffectCompletion,
) -> Result<(), CommitError> {
    let done = match completion.claim_token {
        Some(token) => sqlx::query(
            "UPDATE effects SET status = 'done', completed_at = ? \
                 WHERE id = ? AND status = 'claimed' AND claim_token = ?",
        )
        .bind(ms(completion.at))
        .bind(completion.id.to_string())
        .bind(token.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?,
        None => sqlx::query(
            "UPDATE effects SET status = 'done', completed_at = ? \
             WHERE id = ? AND status = 'pending'",
        )
        .bind(ms(completion.at))
        .bind(completion.id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?,
    };
    if done.rows_affected() == 0 {
        return Err(CommitError::StaleEffect {
            effect: completion.id,
        });
    }
    Ok(())
}
