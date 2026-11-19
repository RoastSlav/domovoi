//! Applying policy writes on one SQLite connection.

use domovoi_core::policy_store::{
    AckRecord, DraftGuard, PolicyWrite, ShadowRecord, ShadowStatus, StoredRule,
};
use domovoi_core::ports::{AggregateRef, CommitError, CommitReceipt, Expectation, Observed};
use domovoi_core::{Actor, PolicyVersionId};
use jiff::Timestamp;
use sqlx::SqliteConnection;

use crate::codec::{int, ms, uint};
use crate::uow::backend;

/// Applies one policy write.
pub(crate) async fn apply(
    conn: &mut SqliteConnection,
    write: &PolicyWrite,
    _receipt: &mut CommitReceipt,
) -> Result<(), CommitError> {
    match write {
        PolicyWrite::SaveDraft {
            id,
            expected,
            based_on,
            rules,
            rules_hash,
            actor,
            at,
        } => {
            let draft = DraftSave {
                id: *id,
                based_on: *based_on,
                rules,
                rules_hash,
                actor,
                at: *at,
            };
            save_draft(conn, *expected, &draft).await
        }
        PolicyWrite::DiscardDraft {
            id,
            expected_revision,
        } => discard_draft(conn, *id, *expected_revision).await,
        PolicyWrite::Publish {
            id,
            expected_revision,
            number,
            acknowledgements,
            actor,
            at,
        } => {
            publish(
                conn,
                *id,
                *expected_revision,
                *number,
                acknowledgements,
                actor,
                *at,
            )
            .await
        }
        PolicyWrite::RecordShadow {
            records, at, error, ..
        } => record_shadow(conn, records, *at, error.as_deref()).await,
    }
}

fn conflict(id: PolicyVersionId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::policy(id),
        expected,
        found,
    }
}

/// The stored draft's id and revision, if there is a draft.
async fn stored_draft(conn: &mut SqliteConnection) -> Result<Option<(String, u64)>, CommitError> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT id, draft_revision FROM policy_versions WHERE status = 'draft'")
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    Ok(row.map(|(id, revision)| (id, uint(revision))))
}

/// Requires the stored draft to be `id` at `revision`.
async fn require_draft(
    conn: &mut SqliteConnection,
    id: PolicyVersionId,
    revision: u64,
) -> Result<(), CommitError> {
    match stored_draft(conn).await? {
        Some((stored, current)) if stored == id.to_string() && current == revision => Ok(()),
        Some((_, current)) => Err(conflict(
            id,
            Expectation::Version(revision),
            Observed::Version(current),
        )),
        None => Err(conflict(
            id,
            Expectation::Version(revision),
            Observed::Missing,
        )),
    }
}

async fn insert_rules(
    conn: &mut SqliteConnection,
    id: PolicyVersionId,
    rules: &[StoredRule],
) -> Result<(), CommitError> {
    for rule in rules {
        sqlx::query(
            "INSERT INTO policy_rules (version_id, rule_id, ordinal, enabled, shadow, locked, body) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&rule.rule_id)
        .bind(i64::from(rule.ordinal))
        .bind(i64::from(rule.enabled))
        .bind(i64::from(rule.shadow))
        .bind(i64::from(rule.locked))
        .bind(&rule.body)
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    }
    Ok(())
}

fn count(rules: &[StoredRule], pick: impl Fn(&StoredRule) -> bool) -> i64 {
    i64::try_from(rules.iter().filter(|rule| pick(rule)).count()).unwrap_or(i64::MAX)
}

/// The parts of a draft save that do not depend on what the store holds.
struct DraftSave<'a> {
    id: PolicyVersionId,
    based_on: Option<u32>,
    rules: &'a [StoredRule],
    rules_hash: &'a str,
    actor: &'a Actor,
    at: Timestamp,
}

async fn save_draft(
    conn: &mut SqliteConnection,
    expected: DraftGuard,
    draft: &DraftSave<'_>,
) -> Result<(), CommitError> {
    let DraftSave {
        id,
        based_on,
        rules,
        rules_hash,
        actor,
        at,
    } = *draft;
    let total = count(rules, |_| true);
    let locked = count(rules, |rule| rule.locked);
    let shadow = count(rules, |rule| rule.shadow);
    match expected {
        DraftGuard::Absent => {
            if let Some((_, current)) = stored_draft(conn).await? {
                return Err(conflict(id, Expectation::New, Observed::Version(current)));
            }
            sqlx::query(
                "INSERT INTO policy_versions (id, status, number, draft_revision, based_on, \
                 rules_hash, rule_count, locked_count, shadow_count, created_at, created_by, \
                 updated_at, updated_by) VALUES (?, 'draft', NULL, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id.to_string())
            .bind(based_on.map(i64::from))
            .bind(rules_hash)
            .bind(total)
            .bind(locked)
            .bind(shadow)
            .bind(ms(at))
            .bind(actor.to_string())
            .bind(ms(at))
            .bind(actor.to_string())
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
            insert_rules(conn, id, rules).await
        }
        DraftGuard::Revision(revision) => {
            require_draft(conn, id, revision).await?;
            sqlx::query("DELETE FROM policy_rules WHERE version_id = ?")
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(backend)?;
            insert_rules(conn, id, rules).await?;
            sqlx::query(
                "UPDATE policy_versions SET draft_revision = draft_revision + 1, rules_hash = ?, \
                 rule_count = ?, locked_count = ?, shadow_count = ?, updated_at = ?, \
                 updated_by = ? WHERE id = ? AND status = 'draft'",
            )
            .bind(rules_hash)
            .bind(total)
            .bind(locked)
            .bind(shadow)
            .bind(ms(at))
            .bind(actor.to_string())
            .bind(id.to_string())
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
            Ok(())
        }
    }
}

async fn discard_draft(
    conn: &mut SqliteConnection,
    id: PolicyVersionId,
    revision: u64,
) -> Result<(), CommitError> {
    require_draft(conn, id, revision).await?;
    sqlx::query("DELETE FROM policy_rules WHERE version_id = ?")
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    sqlx::query("DELETE FROM policy_versions WHERE id = ? AND status = 'draft'")
        .bind(id.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn publish(
    conn: &mut SqliteConnection,
    id: PolicyVersionId,
    revision: u64,
    number: u32,
    acknowledgements: &[AckRecord],
    actor: &Actor,
    at: Timestamp,
) -> Result<(), CommitError> {
    require_draft(conn, id, revision).await?;
    let (highest,): (i64,) = sqlx::query_as("SELECT COALESCE(MAX(number), 0) FROM policy_versions")
        .fetch_one(&mut *conn)
        .await
        .map_err(backend)?;
    let highest = uint(highest);
    if u64::from(number) != highest + 1 {
        return Err(conflict(
            id,
            Expectation::Version(u64::from(number).saturating_sub(1)),
            Observed::Version(highest),
        ));
    }
    sqlx::query(
        "UPDATE policy_versions SET status = 'published', number = ?, published_at = ?, \
         published_by = ?, updated_at = ?, updated_by = ? WHERE id = ? AND status = 'draft'",
    )
    .bind(i64::from(number))
    .bind(ms(at))
    .bind(actor.to_string())
    .bind(ms(at))
    .bind(actor.to_string())
    .bind(id.to_string())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    for ack in acknowledgements {
        sqlx::query(
            "INSERT INTO policy_acknowledgements (version_id, finding_key, kind, first_rule, \
             second_rule, resolution, note, acknowledged_by, acknowledged_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(&ack.finding_key)
        .bind(&ack.kind)
        .bind(&ack.first_rule)
        .bind(&ack.second_rule)
        .bind(ack.resolution.as_deref())
        .bind(&ack.note)
        .bind(ack.acknowledged_by.to_string())
        .bind(ms(ack.acknowledged_at))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    }
    Ok(())
}

async fn record_shadow(
    conn: &mut SqliteConnection,
    records: &[ShadowRecord],
    at: Timestamp,
    error: Option<&str>,
) -> Result<(), CommitError> {
    let mut inserted = [0_u64; 3];
    for record in records {
        let done = sqlx::query(
            "INSERT INTO policy_shadow_decisions (execution_id, operation_id, host_id, contract, \
             step_created_at, evaluated_at, status, reason, policy_version, rules_hash, \
             enforced_outcome, shadow_outcome, degraded, candidate, inputs, enforced, shadow, \
             gaps) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(execution_id) DO NOTHING",
        )
        .bind(record.execution.to_string())
        .bind(record.operation.to_string())
        .bind(record.host.to_string())
        .bind(&record.contract)
        .bind(ms(record.step_created_at))
        .bind(ms(record.evaluated_at))
        .bind(record.status.as_str())
        .bind(record.reason.as_deref())
        .bind(i64::from(record.policy_version))
        .bind(&record.rules_hash)
        .bind(record.enforced_outcome.as_deref())
        .bind(record.shadow_outcome.as_deref())
        .bind(i64::from(record.degraded))
        .bind(&record.candidate)
        .bind(record.inputs.as_deref())
        .bind(record.enforced.as_deref())
        .bind(record.shadow.as_deref())
        .bind(&record.gaps)
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
        if done.rows_affected() > 0 {
            let slot = match record.status {
                ShadowStatus::Evaluated => 0,
                ShadowStatus::Unevaluable => 1,
                ShadowStatus::Skipped => 2,
            };
            inserted[slot] += 1;
        }
    }
    if let Some(error) = error {
        sqlx::query("UPDATE policy_shadow_state SET last_error = ? WHERE id = 1")
            .bind(error)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
    } else {
        sqlx::query(
            "UPDATE policy_shadow_state SET last_sweep_at = ?, last_error = NULL, \
             total_evaluated = total_evaluated + ?, total_unevaluable = total_unevaluable + ?, \
             total_skipped = total_skipped + ? WHERE id = 1",
        )
        .bind(ms(at))
        .bind(int(inserted[0]))
        .bind(int(inserted[1]))
        .bind(int(inserted[2]))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    }
    Ok(())
}
