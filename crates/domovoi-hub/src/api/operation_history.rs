//! How an operation unfolded: a small projection of its journal rows, from which the web UI
//! replays the operation's graph as of an earlier moment. It is a read; it carries ids,
//! timestamps, kind strings and state names, never intent, summaries, reasons or other text.

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::OperationId;
use domovoi_core::ports::{AggregateRef, JournalEntry};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::operations::{off, parse};
use super::snapshot::snapshot_seq;
use super::{ApiState, Authed};

/// The most journal rows one history read returns; `JournalRead::entries_for` clamps to this.
pub const HISTORY_ROW_CAP: u16 = 500;

/// How an operation unfolded, as a projection of its journal rows. Replay rebuilds past states
/// from this and the operation's final trace.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct OperationHistoryDto {
    /// The journal head read **before** the rows: rows with a larger `seq` are not included.
    pub as_of_seq: u64,
    /// True when `events` starts at the `operation.created` row and holds every row up to
    /// `as_of_seq`.
    pub complete: bool,
    /// Why not complete: `"pruned"` (the oldest rows are gone), `"capped"` (more rows than
    /// `HISTORY_ROW_CAP`: the newest are missing) or `"both"`. `null` when complete.
    pub incomplete_reason: Option<String>,
    /// Oldest first, ascending `seq`.
    pub events: Vec<HistoryEventDto>,
}

/// One journal row of the operation, reduced to what replay needs. It never carries intent,
/// summary, reason, output or any free text.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HistoryEventDto {
    /// The row's global journal sequence number.
    pub seq: u64,
    /// When the row was recorded: RFC 3339 UTC with a trailing `Z`.
    pub at: String,
    /// The stored journal kind string; unknown kinds pass through.
    pub kind: String,
    /// The step the row is about, when the payload names one.
    pub step: Option<String>,
    /// The new state the row records: an execution state name for `operation.execution`, an
    /// operation status name for `operation.status`; `null` otherwise.
    pub state: Option<String>,
}

fn text(payload: &Value, field: &str) -> Option<String> {
    payload.get(field)?.as_str().map(str::to_owned)
}

/// Projects one stored row to what replay needs. Reads the payload only when it is a version 1
/// object; anything missing or of the wrong type becomes `None`. Never fails.
#[must_use]
pub fn project_row(entry: &JournalEntry) -> HistoryEventDto {
    let readable = entry.payload_v == 1 && entry.payload.is_object();
    let field = |name: &str| {
        if readable {
            text(&entry.payload, name)
        } else {
            None
        }
    };
    let (step, state) = match entry.kind.as_str() {
        "operation.execution" => (field("step"), field("to")),
        "operation.status" => (None, field("to")),
        "operation.created" => (None, None),
        _ => (field("step"), None),
    };
    HistoryEventDto {
        seq: entry.seq,
        at: entry.occurred_at.to_string(),
        kind: entry.kind.clone(),
        step,
        state,
    }
}

/// Shapes the answer from the rows read: drops rows committed after `as_of`, and says why the
/// history is incomplete when its head was pruned or the read hit the cap.
#[must_use]
pub fn build_history(
    as_of: u64,
    mut rows: Vec<JournalEntry>,
    row_cap: usize,
) -> OperationHistoryDto {
    // Exactly the cap cannot be told from "more": treat it as capped.
    let capped = rows.len() >= row_cap;
    rows.retain(|row| row.seq <= as_of);
    let pruned = rows
        .first()
        .is_none_or(|row| row.kind != "operation.created");
    let reason = match (pruned, capped) {
        (false, false) => None,
        (true, false) => Some("pruned"),
        (false, true) => Some("capped"),
        (true, true) => Some("both"),
    };
    OperationHistoryDto {
        as_of_seq: as_of,
        complete: reason.is_none(),
        incomplete_reason: reason.map(str::to_owned),
        events: rows.iter().map(project_row).collect(),
    }
}

/// How one operation unfolded, for replaying its graph.
#[utoipa::path(
    get, path = "/api/v1/operations/{id}/history", operation_id = "get_operation_history", tag = "operations",
    params(("id" = String, Path, description = "Operation id")),
    responses(
        (status = 200, description = "The operation's journal rows, reduced", body = OperationHistoryDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such operation", body = ErrorBody),
        (status = 503, description = "Orchestration is not enabled", body = ErrorBody),
    )
)]
pub(super) async fn get_operation_history(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<OperationHistoryDto>, ApiError> {
    if !state.services().operations.enabled() {
        return Err(off());
    }
    let id: OperationId = parse(&id)?;
    let as_of = snapshot_seq(&state).await?;
    state
        .deps
        .operation_repo
        .load(id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let rows = state
        .deps
        .journal
        .entries_for(&AggregateRef::operation(id), HISTORY_ROW_CAP)
        .await
        .map_err(|err| ApiError::internal("reading the operation's history", err))?;
    Ok(Json(build_history(
        as_of,
        rows,
        usize::from(HISTORY_ROW_CAP),
    )))
}

#[cfg(test)]
mod tests {
    use domovoi_core::OperationId;
    use serde_json::{Value, json};

    use super::*;

    fn row(seq: u64, kind: &str, payload_v: u32, payload: Value) -> JournalEntry {
        JournalEntry {
            seq,
            aggregate: AggregateRef::operation(OperationId::new()),
            kind: kind.to_owned(),
            payload_v,
            payload,
            actor: "test".to_owned(),
            occurred_at: "2026-01-01T00:00:02Z".parse().unwrap(),
        }
    }

    fn pair(entry: &JournalEntry) -> (Option<String>, Option<String>) {
        let event = project_row(entry);
        (event.step, event.state)
    }

    fn some(a: &str) -> Option<String> {
        Some(a.to_owned())
    }

    #[test]
    fn project_row_reads_only_version_1_object_payloads() {
        let step = json!({"step": "s1", "to": "dispatched", "intent": "SECRET"});
        for kind in [
            "operation.step_created",
            "operation.step_started",
            "operation.step_finished",
        ] {
            assert_eq!(pair(&row(1, kind, 1, step.clone())), (some("s1"), None));
        }
        assert_eq!(
            pair(&row(1, "operation.execution", 1, step.clone())),
            (some("s1"), some("dispatched"))
        );
        assert_eq!(
            pair(&row(1, "operation.status", 1, step.clone())),
            (None, some("dispatched"))
        );
        assert_eq!(
            pair(&row(1, "operation.created", 1, step.clone())),
            (None, None)
        );
        assert_eq!(
            pair(&row(1, "operation.future_thing", 1, step.clone())),
            (some("s1"), None)
        );
        assert_eq!(
            project_row(&row(7, "operation.created", 1, json!({}))).at,
            "2026-01-01T00:00:02Z"
        );

        assert_eq!(
            pair(&row(1, "operation.execution", 2, step.clone())),
            (None, None)
        );
        assert_eq!(
            pair(&row(1, "operation.execution", 1, json!("text"))),
            (None, None)
        );
        assert_eq!(
            pair(&row(
                1,
                "operation.execution",
                1,
                json!({"step": 5, "to": ["x"]})
            )),
            (None, None)
        );
        assert_eq!(
            pair(&row(1, "operation.step_created", 1, Value::Null)),
            (None, None)
        );
    }

    #[test]
    fn build_history_drops_rows_after_the_snapshot_and_reports_gaps() {
        let created = || row(1, "operation.created", 1, json!({}));
        let step = |seq| row(seq, "operation.step_created", 1, json!({"step": "s"}));

        let whole = build_history(3, vec![created(), step(2), step(3), step(4)], 500);
        assert!(whole.complete);
        assert_eq!(whole.incomplete_reason, None);
        assert_eq!(whole.as_of_seq, 3);
        assert_eq!(
            whole.events.len(),
            3,
            "the row after the snapshot is not part of it"
        );

        let pruned = build_history(9, vec![step(2), step(3)], 500);
        assert_eq!(pruned.incomplete_reason.as_deref(), Some("pruned"));
        let empty = build_history(9, Vec::new(), 500);
        assert_eq!(
            (empty.complete, empty.incomplete_reason.as_deref()),
            (false, Some("pruned"))
        );
        let capped = build_history(9, vec![created(), step(2)], 2);
        assert_eq!(capped.incomplete_reason.as_deref(), Some("capped"));
        let both = build_history(9, vec![step(2), step(3)], 2);
        assert_eq!(both.incomplete_reason.as_deref(), Some("both"));
    }
}
