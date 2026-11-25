//! Applying one [`PermitWrite`] on one SQLite connection.

use domovoi_core::ports::{CommitError, PermitWrite};
use sqlx::SqliteConnection;

use crate::codec::ms;
use crate::uow::backend;

/// Applies one autonomy write.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &PermitWrite,
) -> Result<(), CommitError> {
    match write {
        PermitWrite::Pause { by, reason, at } => {
            sqlx::query(
                "UPDATE autonomy_state SET paused = 1, changed_by = ?, changed_at = ?, \
                 reason = ? WHERE id = 1",
            )
            .bind(by)
            .bind(ms(*at))
            .bind(reason)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
        }
        PermitWrite::Resume { by, at } => {
            sqlx::query(
                "UPDATE autonomy_state SET paused = 0, changed_by = ?, changed_at = ?, \
                 reason = NULL WHERE id = 1",
            )
            .bind(by)
            .bind(ms(*at))
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
        }
        PermitWrite::ResetRule { rule, by, at } => {
            sqlx::query(
                "INSERT INTO rule_circuit_resets (rule_id, reset_at, reset_by) VALUES (?, ?, ?)",
            )
            .bind(rule)
            .bind(ms(*at))
            .bind(by)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
        }
        PermitWrite::ResolveUnknown {
            execution,
            by,
            how,
            note,
            at,
        } => {
            let result = sqlx::query(
                "INSERT INTO unknown_resolutions (execution_id, resolved_at, resolved_by, how, \
                 note) VALUES (?, ?, ?, ?, ?) ON CONFLICT (execution_id) DO NOTHING",
            )
            .bind(execution.to_string())
            .bind(ms(*at))
            .bind(by)
            .bind(how.as_str())
            .bind(note)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
            if result.rows_affected() == 0 {
                return Err(CommitError::Constraint(
                    "this unknown outcome was already resolved".to_owned(),
                ));
            }
        }
    }
    Ok(())
}
