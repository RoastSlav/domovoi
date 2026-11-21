//! The audit log for the web UI: entries with their chain fields, and verification. Both
//! routes only read.

use axum::Json;
use axum::extract::{Query, State};

use domovoi_core::audit_chain::VerifyScope;
use domovoi_core::ports::{AuditFilter, AuditHistory, AuditRowFilter};

use super::dto_audit::{
    AuditPageDto, AuditQuery, AuditVerificationDto, VerificationQuery, row_dto, verification_dto,
};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, page};

/// How many of the newest chained entries a `recent` check covers: the same bound the hub
/// uses when it starts.
pub const RECENT_ROWS: u64 = 10_000;

/// The longest `kind` filter accepted.
const MAX_KIND: usize = 100;

fn shown(text: &str) -> String {
    text.chars().take(40).collect()
}

/// Audit entries, newest first, with their hash.
#[utoipa::path(
    get,
    path = "/api/v1/audit",
    operation_id = "list_audit",
    tag = "audit",
    params(AuditQuery),
    responses(
        (status = 200, description = "A page of audit entries", body = AuditPageDto),
        (status = 400, description = "Unknown history or a kind that is too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditPageDto>, ApiError> {
    let history = match query.history.as_deref() {
        None => None,
        Some("legacy") => Some(AuditHistory::Legacy),
        Some("chained") => Some(AuditHistory::Chained),
        Some(other) => {
            return Err(ApiError::BadRequest(format!(
                "unknown history `{}`; use legacy or chained",
                shown(other)
            )));
        }
    };
    if query
        .kind
        .as_deref()
        .is_some_and(|k| k.chars().count() > MAX_KIND)
    {
        return Err(ApiError::BadRequest("kind is too long".to_owned()));
    }
    let inspect = &state.deps.audit_inspect;
    let legacy_last_id = inspect
        .anchor()
        .await
        .map_err(|e| ApiError::internal("reading the audit log", e))?
        .map_or(0, |anchor| anchor.legacy_last_id);
    let filter = AuditRowFilter {
        events: AuditFilter {
            kind: query.kind,
            ..AuditFilter::default()
        },
        history,
    };
    let rows = inspect
        .rows(&filter, &page(query.cursor, query.limit))
        .await
        .map_err(|e| ApiError::internal("reading the audit log", e))?;
    let redactor = &*state.deps.redactor;
    Ok(Json(AuditPageDto {
        legacy_last_id,
        next_cursor: rows.next_cursor,
        items: rows
            .items
            .iter()
            .map(|row| row_dto(row, legacy_last_id, redactor))
            .collect(),
    }))
}

/// Checks the audit log: the legacy and the chained history separately.
#[utoipa::path(
    get,
    path = "/api/v1/audit/verification",
    operation_id = "verify_audit",
    tag = "audit",
    params(VerificationQuery),
    responses(
        (status = 200, description = "The result of the check", body = AuditVerificationDto),
        (status = 400, description = "Unknown scope", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn verification(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<VerificationQuery>,
) -> Result<Json<AuditVerificationDto>, ApiError> {
    let (name, scope) = match query.scope.as_deref() {
        None | Some("recent") => ("recent", VerifyScope::Last(RECENT_ROWS)),
        Some("full") => ("full", VerifyScope::Full),
        Some(other) => {
            return Err(ApiError::BadRequest(format!(
                "unknown scope `{}`; use recent or full",
                shown(other)
            )));
        }
    };
    let inspect = &state.deps.audit_inspect;
    let report = inspect
        .verify(scope)
        .await
        .map_err(|e| ApiError::internal("verifying the audit log", e))?;
    let head = inspect
        .head()
        .await
        .map_err(|e| ApiError::internal("verifying the audit log", e))?;
    let checked_at = state.deps.clock.now().to_string();
    Ok(Json(verification_dto(
        &report,
        name,
        RECENT_ROWS,
        checked_at,
        head,
    )))
}

#[cfg(test)]
mod tests {
    use super::super::dto_audit::{MAX_DETAIL_BYTES, row_dto};
    use domovoi_core::audit_chain::ChainRow;
    use domovoi_core::ports::{AuditEvent, Redactor};
    use jiff::Timestamp;
    use serde_json::Value;

    struct Plain;
    impl Redactor for Plain {
        fn redact(&self, text: &str) -> String {
            text.to_owned()
        }
    }

    fn row(seq: u64, hash: Option<[u8; 32]>, detail: Value) -> ChainRow {
        ChainRow {
            seq,
            prev_hash: None,
            hash,
            event: AuditEvent {
                at: Timestamp::UNIX_EPOCH,
                actor: "a".to_owned(),
                kind: "k".to_owned(),
                subject: "s".to_owned(),
                detail,
            },
        }
    }

    #[test]
    fn row_dto_marks_rows_up_to_the_anchor_as_legacy_whatever_their_hash() {
        let a = row_dto(&row(3, None, Value::Null), 5, &Plain);
        let b = row_dto(&row(6, Some([1; 32]), Value::Null), 5, &Plain);
        let c = row_dto(&row(6, None, Value::Null), 5, &Plain);
        assert_eq!((a.history.as_str(), a.hash), ("legacy", None));
        assert_eq!(b.history, "chained");
        assert_eq!(b.hash, Some("01".repeat(32)));
        assert_eq!(c.history, "chained");
        assert_eq!(c.hash, None);
    }

    #[test]
    fn row_dto_cuts_detail_on_a_character_boundary() {
        let long = Value::String("é".repeat(MAX_DETAIL_BYTES));
        let dto = row_dto(&row(1, None, long), 0, &Plain);
        assert!(dto.detail_truncated);
        assert!(dto.detail.len() <= MAX_DETAIL_BYTES);
        let none = row_dto(&row(1, None, Value::Null), 0, &Plain);
        assert_eq!(none.detail, "");
    }
}
