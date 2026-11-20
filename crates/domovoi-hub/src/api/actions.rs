//! Actions: the read-only list of retired legacy actions. Approving happens on operation
//! steps (INV-1).

use axum::Json;
use axum::extract::{Query, State};
use domovoi_core::ActionStatus;
use domovoi_core::ports::ActionFilter;

use super::dto::{ActionDto, ActionPage, ActionQuery};
use super::error::{ApiError, ErrorBody};
use super::snapshot::snapshot_seq;
use super::{ApiState, Authed, host_named, page};

/// Actions, newest first.
#[utoipa::path(
    get, path = "/api/v1/actions", operation_id = "list_actions", tag = "actions",
    params(ActionQuery),
    responses(
        (status = 200, description = "One page", body = ActionPage),
        (status = 400, description = "Unknown status, or a time that does not parse", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<ActionQuery>,
) -> Result<Json<ActionPage>, ApiError> {
    let as_of_seq = snapshot_seq(&state).await?;
    if let Some(status) = &query.status
        && !ActionStatus::NAMES.contains(&status.as_str())
    {
        return Err(ApiError::BadRequest(format!("unknown status `{status}`")));
    }
    let host = match &query.host {
        Some(name) => Some(host_named(&state, name).await?.id),
        None => None,
    };
    let updated_since = match &query.since {
        Some(since) => Some(
            since
                .parse()
                .map_err(|_| ApiError::BadRequest(format!("`{since}` is not a time")))?,
        ),
        None => None,
    };
    let filter = ActionFilter {
        host,
        status: query.status.clone(),
        decided: query.decided,
        updated_since,
        ..ActionFilter::default()
    };
    let got = state
        .deps
        .actions
        .list(&filter, &page(query.cursor, query.limit))
        .await?;
    Ok(Json(ActionPage {
        items: got.items.iter().map(ActionDto::from).collect(),
        next_cursor: got.next_cursor,
        as_of_seq,
    }))
}
