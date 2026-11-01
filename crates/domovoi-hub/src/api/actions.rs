//! Actions: list, approve and deny. Approval is the web surface's path to `Approved`
//! (INV-1); destructive skills need the skill's name typed exactly (INV-6).

use axum::Json;
use axum::extract::{Path, Query, State};
use domovoi_core::ports::ActionFilter;
use domovoi_core::{ActionError, ActionId, ActionRequest, ActionStatus};
use domovoi_engine::ApprovalError;

use super::dto::{ActionDto, ActionPage, ActionQuery, ApproveRequest};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, page};
use crate::auth::approval::web_approval;

fn parse_id(id: &str) -> Result<ActionId, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound)
}

fn approval_error(err: ApprovalError) -> ApiError {
    match err {
        ApprovalError::NotFound(_) => ApiError::NotFound,
        ApprovalError::BadToken => ApiError::Forbidden(err.to_string()),
        ApprovalError::Refused(ActionError::DestructiveNeedsTypedConfirmation) => {
            ApiError::Forbidden("type the skill's name to approve a destructive action".to_owned())
        }
        ApprovalError::Refused(refused) => ApiError::Conflict(refused.to_string()),
        ApprovalError::Store(e) => ApiError::internal("approval failed", e),
    }
}

async fn load(state: &ApiState, id: ActionId) -> Result<ActionRequest, ApiError> {
    state.deps.actions.get(id).await?.ok_or(ApiError::NotFound)
}

/// Actions, newest first.
#[utoipa::path(
    get, path = "/api/v1/actions", tag = "actions",
    params(ActionQuery),
    responses(
        (status = 200, description = "One page", body = ActionPage),
        (status = 400, description = "Unknown status", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<ActionQuery>,
) -> Result<Json<ActionPage>, ApiError> {
    if let Some(status) = &query.status
        && !ActionStatus::NAMES.contains(&status.as_str())
    {
        return Err(ApiError::BadRequest(format!("unknown status `{status}`")));
    }
    let filter = ActionFilter {
        status: query.status.clone(),
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
    }))
}

/// Approves a proposed action. Destructive skills need `confirm_text` equal to the skill's
/// name; chat buttons can never approve them.
#[utoipa::path(
    post, path = "/api/v1/actions/{id}/approve", tag = "actions",
    params(("id" = String, Path, description = "Action id")),
    request_body = ApproveRequest,
    responses(
        (status = 200, description = "Approved; it runs next", body = ActionDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Destructive without the typed confirmation", body = ErrorBody),
        (status = 404, description = "No such action", body = ErrorBody),
        (status = 409, description = "Not proposed any more, or expired", body = ErrorBody),
    )
)]
pub(super) async fn approve(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
    Json(req): Json<ApproveRequest>,
) -> Result<Json<ActionDto>, ApiError> {
    let action = load(&state, parse_id(&id)?).await?;
    let approval = web_approval(
        current.user.id,
        req.confirm_text.as_deref(),
        action.skill(),
        state.deps.clock.now(),
    );
    let approved = state
        .deps
        .actions_service
        .approve(action.id(), approval, None)
        .await
        .map_err(approval_error)?;
    Ok(Json(ActionDto::from(&approved)))
}

/// Denies a proposed action.
#[utoipa::path(
    post, path = "/api/v1/actions/{id}/deny", tag = "actions",
    params(("id" = String, Path, description = "Action id")),
    responses(
        (status = 200, description = "Denied", body = ActionDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such action", body = ErrorBody),
        (status = 409, description = "Not proposed any more", body = ErrorBody),
    )
)]
pub(super) async fn deny(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
) -> Result<Json<ActionDto>, ApiError> {
    let action = load(&state, parse_id(&id)?).await?;
    let approval = web_approval(
        current.user.id,
        None,
        action.skill(),
        state.deps.clock.now(),
    );
    let denied = state
        .deps
        .actions_service
        .deny(action.id(), approval)
        .await
        .map_err(approval_error)?;
    Ok(Json(ActionDto::from(&denied)))
}
