//! `GET /readyz`: whether the hub can do its job, as opposed to whether it serves HTTP.
//!
//! Every check runs even when an earlier one fails, and the answer names only the checks
//! that failed. Error text goes to the log, never to the caller.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Serialize;
use utoipa::ToSchema;

use super::ApiState;

/// The store can take a write transaction.
pub const STORE_WRITABLE: &str = "store_writable";
/// Every embedded migration is applied.
pub const MIGRATIONS: &str = "migrations";
/// The incident manager task is running.
pub const INCIDENT_MANAGER: &str = "incident_manager";
/// The action service task is running.
pub const ACTION_SERVICE: &str = "action_service";

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
    let body = check(&state).await;
    let status = if body.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(body))
}

/// Runs every check and collects the names that failed.
async fn check(state: &ApiState) -> ReadyzBody {
    let deps = &state.deps;
    let mut failing = Vec::new();
    if let Err(err) = deps.store_health.writable().await {
        tracing::warn!(%err, "readiness: the store is not writable");
        failing.push(STORE_WRITABLE.to_owned());
    }
    match deps.store_health.migrations_current().await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!("readiness: not every migration is applied");
            failing.push(MIGRATIONS.to_owned());
        }
        Err(err) => {
            tracing::warn!(%err, "readiness: the migration history could not be read");
            failing.push(MIGRATIONS.to_owned());
        }
    }
    if !deps.manager.is_alive() {
        tracing::warn!("readiness: the incident manager is not running");
        failing.push(INCIDENT_MANAGER.to_owned());
    }
    if !deps.actions_service.is_alive() {
        tracing::warn!("readiness: the action service is not running");
        failing.push(ACTION_SERVICE.to_owned());
    }
    ReadyzBody {
        ready: failing.is_empty(),
        failing,
    }
}
