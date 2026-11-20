//! Past operations that resemble a situation.

use std::collections::BTreeSet;

use domovoi_core::OperationId;
use domovoi_core::experience::{
    SIMILAR_MIN_SCORE, SimilarHit, SimilarQuery, order_similar, query_words, score_similar,
};
use domovoi_core::ports::StoreError;

use super::rows::load_summaries;
use super::search::scope_clause;
use crate::codec::parse;
use crate::db::Store;
use crate::error::db;
use crate::fts::match_any_in;

/// Most subject candidates read.
const SUBJECT_CANDIDATES: i64 = 100;
/// Most text candidates read.
const WORD_CANDIDATES: i64 = 20;

pub(super) async fn similar(
    store: &Store,
    query: &SimilarQuery,
) -> Result<Vec<SimilarHit>, StoreError> {
    let host = query.host.to_string();
    let exclude = query.exclude.map(|id| id.to_string());
    let check = query.check.as_ref().map(ToString::to_string);
    let target = query.target.as_ref().map(ToString::to_string);

    let sql = format!(
        "SELECT DISTINCT r.operation_id, r.ended_at FROM operation_resolution_subjects s \
         JOIN operation_resolutions r ON r.operation_id = s.operation_id \
         WHERE s.host_id = ?1 AND {} AND (?4 IS NULL OR r.operation_id <> ?4) \
         AND ((?2 IS NOT NULL AND s.check_key = ?2) OR (?3 IS NOT NULL AND s.target = ?3)) \
         ORDER BY r.ended_at DESC, r.operation_id DESC LIMIT ?5",
        scope_clause(1)
    );
    let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(&host)
        .bind(&check)
        .bind(&target)
        .bind(&exclude)
        .bind(SUBJECT_CANDIDATES)
        .fetch_all(&store.read)
        .await
        .map_err(db)?;
    let mut ids: Vec<String> = rows.into_iter().map(|(id, _)| id).collect();

    let mut ranked: Vec<String> = Vec::new();
    if let Some(expression) = match_any_in(
        &["goal", "problem", "cause"],
        &query_words(&query.text).join(" "),
    ) {
        let sql = format!(
            "SELECT r.operation_id FROM operation_resolutions_fts \
             JOIN operation_resolutions r ON r.rowid = operation_resolutions_fts.rowid \
             WHERE operation_resolutions_fts MATCH ?1 AND {} AND (?3 IS NULL OR r.operation_id <> ?3) \
             ORDER BY -bm25(operation_resolutions_fts) DESC, r.ended_at DESC, r.operation_id DESC \
             LIMIT ?4",
            scope_clause(2)
        );
        let rows: Vec<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(expression)
            .bind(&host)
            .bind(&exclude)
            .bind(WORD_CANDIDATES)
            .fetch_all(&store.read)
            .await
            .map_err(db)?;
        ranked = rows.into_iter().map(|(id,)| id).collect();
    }
    for id in &ranked {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    let parsed = ids
        .iter()
        .map(|id| parse::<OperationId>(id, "operation id"))
        .collect::<Result<BTreeSet<_>, _>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let summaries = load_summaries(&store.read, &parsed).await?;
    let mut hits = Vec::new();
    for (id, summary) in summaries {
        let rank = ranked.iter().position(|r| *r == id.to_string());
        let (score, reasons) = score_similar(query, &summary, rank);
        if score >= SIMILAR_MIN_SCORE {
            hits.push(SimilarHit {
                summary,
                score,
                reasons,
            });
        }
    }
    order_similar(&mut hits);
    hits.truncate(query.limit());
    Ok(hits)
}
