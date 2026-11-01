//! Advisories, the digest, `/healthz` and `/metrics`.

use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use domovoi_core::ports::{IncidentFilter, OutboundMessage, Page};
use domovoi_engine::digest::{DigestInputs, build_digest};
use jiff::SignedDuration;

use super::dto_admin::{AdvisoryDto, DigestPreview};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed};

/// Days of facts before today that trends are computed from.
pub const HISTORY_DAYS: i64 = 14;

/// Open advisories, newest first.
#[utoipa::path(
    get, path = "/api/v1/advisories", operation_id = "list_advisories", tag = "system",
    responses(
        (status = 200, description = "Open advisories", body = Vec<AdvisoryDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn advisories(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<Vec<AdvisoryDto>>, ApiError> {
    let open = state.deps.advisories.list_open().await?;
    Ok(Json(
        open.iter()
            .map(|a| AdvisoryDto {
                id: a.id.to_string(),
                kind: serde_json::to_value(a.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                host_id: a.host.map(|h| h.to_string()),
                title: a.title.clone(),
                detail: a.detail.clone(),
                created_at: a.created_at.to_string(),
            })
            .collect(),
    ))
}

/// Today's digest, built now.
async fn build(state: &ApiState) -> Result<DigestPreview, ApiError> {
    let deps = &state.deps;
    let tz = deps.config.hub.timezone.zone().clone();
    let now = deps.clock.now();
    let date = now.to_zoned(tz.clone()).date();
    let start = date
        .to_zoned(tz.clone())
        .map_err(|e| ApiError::internal("digest day start", e))?
        .timestamp();
    let history_start = start
        .checked_sub(SignedDuration::from_hours(24 * HISTORY_DAYS))
        .unwrap_or(start);
    let hosts = deps.hosts.list_hosts().await?;
    let mut facts_today = Vec::new();
    let mut facts_history = Vec::new();
    for host in &hosts {
        facts_today.extend(deps.facts.list(host.id, start, now).await?);
        facts_history.extend(deps.facts.list(host.id, history_start, start).await?);
    }
    let active = IncidentFilter {
        active: Some(true),
        ..IncidentFilter::default()
    };
    let open_incidents = deps
        .incidents
        .list(&active, &Page::first(Page::MAX_LIMIT))
        .await?
        .items;
    let advisories = deps.advisories.list_open().await?;
    let digest = build_digest(&DigestInputs {
        date,
        hosts: &hosts,
        facts_today: &facts_today,
        facts_history: &facts_history,
        open_incidents: &open_incidents,
        advisories: &advisories,
        tz: &tz,
        now,
    });
    Ok(DigestPreview {
        date: date.to_string(),
        markdown: domovoi_engine::render::digest(&digest),
    })
}

/// Today's digest as it would be sent now.
#[utoipa::path(
    get, path = "/api/v1/digest/preview", operation_id = "preview_digest", tag = "system",
    responses(
        (status = 200, description = "The digest", body = DigestPreview),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn digest_preview(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<DigestPreview>, ApiError> {
    Ok(Json(build(&state).await?))
}

/// Sends today's digest now.
#[utoipa::path(
    post, path = "/api/v1/digest/send", operation_id = "send_digest", tag = "system",
    responses(
        (status = 200, description = "Sent", body = DigestPreview),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 503, description = "No notifier is configured, or sending failed", body = ErrorBody),
    )
)]
pub(super) async fn digest_send(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<DigestPreview>, ApiError> {
    let notifier = state
        .deps
        .notifier
        .clone()
        .ok_or_else(|| ApiError::Unavailable("no notifier is configured".to_owned()))?;
    let digest = build(&state).await?;
    let message = OutboundMessage {
        title: format!("Daily digest for {}", digest.date),
        markdown: digest.markdown.clone(),
        severity: None,
    };
    notifier
        .send(&message)
        .await
        .map_err(|e| ApiError::Unavailable(format!("sending failed: {e}")))?;
    Ok(Json(digest))
}

/// Liveness: `ok` while the hub serves requests.
#[utoipa::path(
    get, path = "/healthz", operation_id = "healthz", tag = "system",
    responses((status = 200, description = "Healthy", body = String, content_type = "text/plain"))
)]
pub(super) async fn healthz() -> &'static str {
    "ok"
}

/// Prometheus metrics.
#[utoipa::path(
    get, path = "/metrics", operation_id = "metrics", tag = "system",
    responses((status = 200, description = "Prometheus text format", body = String, content_type = "text/plain"))
)]
pub(super) async fn metrics(State(state): State<ApiState>) -> impl IntoResponse {
    let body = state
        .deps
        .metrics
        .as_ref()
        .map(|render| render())
        .unwrap_or_default();
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}
