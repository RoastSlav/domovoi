//! The filtered and paged reads of decision rows.

use std::collections::BTreeMap;

use domovoi_core::policy_store::{ShadowFilter, ShadowRecord, ShadowSummary};
use domovoi_core::ports::{Page, Paged, StoreError};
use jiff::Timestamp;
use sqlx::SqlitePool;

use super::rows::{ShadowRow, shadow_columns};
use crate::codec::{Cursor, fetch_limit, ms, paginate, uint};
use crate::error::db;

/// Decision rows matching `filter`, newest `evaluated_at` first, keyset-paged over
/// `(evaluated_at, execution_id)`.
pub(crate) async fn shadow_records(
    pool: &SqlitePool,
    filter: &ShadowFilter,
    page: &Page,
) -> Result<Paged<ShadowRecord>, StoreError> {
    const SQL: &str = concat!(
        shadow_columns!(),
        " WHERE (?1 IS NULL OR host_id = ?1) \
        AND (?2 IS NULL OR contract = ?2) \
        AND (?3 IS NULL OR status = ?3) \
        AND (?4 IS NULL OR enforced_outcome = ?4) \
        AND (?5 = 0 OR enforced_outcome IS NOT shadow_outcome) \
        AND (?6 IS NULL OR evaluated_at >= ?6) \
        AND (?7 IS NULL OR evaluated_at < ?7) \
        AND (?8 IS NULL OR evaluated_at < ?8 OR (evaluated_at = ?8 AND execution_id < ?9)) \
        ORDER BY evaluated_at DESC, execution_id DESC LIMIT ?10"
    );
    let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
    let limit = page.effective_limit();
    let rows: Vec<ShadowRow> = sqlx::query_as(SQL)
        .bind(filter.host.map(|host| host.to_string()))
        .bind(filter.contract.as_deref())
        .bind(filter.status.map(|status| status.as_str()))
        .bind(filter.enforced_outcome.as_deref())
        .bind(i64::from(filter.differing))
        .bind(filter.since.map(ms))
        .bind(filter.until.map(ms))
        .bind(after_key)
        .bind(after_id)
        .bind(fetch_limit(limit))
        .fetch_all(pool)
        .await
        .map_err(db)?;
    let records = rows
        .into_iter()
        .map(ShadowRow::into_record)
        .collect::<Result<Vec<_>, _>>()?;
    let (records, next_cursor) = paginate(records, limit, |record| Cursor {
        key: ms(record.evaluated_at),
        id: record.execution.to_string(),
    });
    Ok(Paged {
        items: records,
        next_cursor,
    })
}

/// Counts of the rows evaluated at or after `since`.
pub(crate) async fn shadow_summary(
    pool: &SqlitePool,
    since: Timestamp,
) -> Result<ShadowSummary, StoreError> {
    let (total, degraded, differing): (i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT count(*), sum(degraded), \
         sum(CASE WHEN enforced_outcome IS NOT shadow_outcome THEN 1 ELSE 0 END) \
         FROM policy_shadow_decisions WHERE evaluated_at >= ?",
    )
    .bind(ms(since))
    .fetch_one(pool)
    .await
    .map_err(db)?;
    let by_status: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, count(*) FROM policy_shadow_decisions WHERE evaluated_at >= ? \
         GROUP BY status",
    )
    .bind(ms(since))
    .fetch_all(pool)
    .await
    .map_err(db)?;
    let by_outcome: Vec<(String, i64)> = sqlx::query_as(
        "SELECT enforced_outcome, count(*) FROM policy_shadow_decisions \
         WHERE evaluated_at >= ? AND enforced_outcome IS NOT NULL GROUP BY enforced_outcome",
    )
    .bind(ms(since))
    .fetch_all(pool)
    .await
    .map_err(db)?;
    let counts = |rows: Vec<(String, i64)>| -> BTreeMap<String, u64> {
        rows.into_iter()
            .map(|(name, count)| (name, uint(count)))
            .collect()
    };
    Ok(ShadowSummary {
        since,
        total: uint(total),
        by_status: counts(by_status),
        by_enforced_outcome: counts(by_outcome),
        degraded: uint(degraded.unwrap_or(0)),
        differing: uint(differing.unwrap_or(0)),
    })
}
