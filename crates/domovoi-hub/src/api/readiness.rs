//! `GET /readyz`: whether the hub can do its job, as opposed to whether it serves HTTP.
//!
//! Every check runs even when an earlier one fails, and the answer names only the checks
//! that failed. Error text goes to the log, never to the caller.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use serde::Serialize;
use utoipa::ToSchema;

use domovoi_core::ports::StoreError;

use super::ApiState;

/// The store can take a write transaction.
pub const STORE_WRITABLE: &str = "store_writable";
/// Every embedded migration is applied.
pub const MIGRATIONS: &str = "migrations";
/// The incident manager task is running.
pub const INCIDENT_MANAGER: &str = "incident_manager";

/// The header of a 503 `/readyz` answer that says, in a sentence per failing check, why.
pub const NOT_READY_HEADER: HeaderName = HeaderName::from_static("x-domovoi-not-ready");

/// The body of `/readyz`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReadyzBody {
    /// True when no check failed.
    pub ready: bool,
    /// The names of the checks that failed, in a fixed order; empty when ready.
    pub failing: Vec<String>,
}

/// Readiness: 200 when the hub can do its job, 503 naming what it cannot.
#[utoipa::path(
    get, path = "/readyz", operation_id = "readyz", tag = "system",
    responses(
        (status = 200, description = "Ready", body = ReadyzBody),
        (status = 503, description = "Not ready; the body names the failing checks", body = ReadyzBody),
    )
)]
pub(super) async fn readyz(State(state): State<ApiState>) -> impl IntoResponse {
    let (body, reasons) = check(&state).await;
    let status = if body.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let mut response = (status, Json(body)).into_response();
    // The same sentence for the same check, never the error's own text: a person reading
    // `curl -i` sees why without the response disclosing anything about the host.
    if !reasons.is_empty()
        && let Ok(value) = HeaderValue::from_str(&reasons.join("; "))
    {
        response.headers_mut().insert(NOT_READY_HEADER, value);
    }
    response
}

/// Runs every check and collects the names that failed.
async fn check(state: &ApiState) -> (ReadyzBody, Vec<&'static str>) {
    let deps = &state.deps;
    let mut failing = Vec::new();
    let mut reasons = Vec::new();
    if let Err(err) = deps.store_health.writable().await {
        match err {
            StoreError::Unwritable(cause) => {
                super::error::log_store_unwritable(&cause);
                reasons.push("store_writable: the hub cannot write to its database (the disk is full or read-only)");
            }
            other => {
                tracing::warn!(err = %other, "readiness: the store is not writable");
                reasons.push("store_writable: the hub's database did not accept a write");
            }
        }
        failing.push(STORE_WRITABLE.to_owned());
    }
    match deps.store_health.migrations_current().await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!("readiness: not every migration is applied");
            reasons.push("migrations: not every database migration is applied");
            failing.push(MIGRATIONS.to_owned());
        }
        Err(err) => {
            tracing::warn!(%err, "readiness: the migration history could not be read");
            reasons.push("migrations: the migration history could not be read");
            failing.push(MIGRATIONS.to_owned());
        }
    }
    if !deps.manager.is_alive() {
        tracing::warn!("readiness: the incident manager is not running");
        reasons.push("incident_manager: the incident manager is not running");
        failing.push(INCIDENT_MANAGER.to_owned());
    }
    (
        ReadyzBody {
            ready: failing.is_empty(),
            failing,
        },
        reasons,
    )
}
