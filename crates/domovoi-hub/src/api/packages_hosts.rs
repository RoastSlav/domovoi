//! What a host reports installed, and asking its agent again.

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::{HostId, HostPackage};

use super::dto_packages::{HostPackageDto, HostPackagesDto};
use super::error::{ApiError, ErrorBody};
use super::packages_error::api_error;
use super::{ApiState, Authed, host_named};

fn dto(host: HostId, rows: &[HostPackage]) -> HostPackagesDto {
    HostPackagesDto {
        host_id: host.to_string(),
        observed_at: rows
            .iter()
            .map(|row| row.observed_at)
            .max()
            .map(|at| at.to_string()),
        entries: rows.iter().map(HostPackageDto::from).collect(),
        install_allowed: None,
    }
}

/// The packages the hub last saw installed on host `name`.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/packages", operation_id = "list_host_packages", tag = "packages",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The host's packages", body = HostPackagesDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<HostPackagesDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let rows = state
        .services()
        .packages
        .host_packages(host.id)
        .await
        .map_err(api_error)?;
    Ok(Json(dto(host.id, &rows)))
}

/// Asks host `name`'s agent which packages it has and replaces what the hub holds with the
/// answer. Needs the agent connected.
#[utoipa::path(
    post, path = "/api/v1/hosts/{name}/packages/refresh", operation_id = "refresh_host_packages", tag = "packages",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The host's packages as the agent reported them", body = HostPackagesDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "Packages are disabled, or the agent is not connected or did not answer", body = ErrorBody),
    )
)]
pub(super) async fn refresh(
    State(state): State<ApiState>,
    auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<HostPackagesDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let rows = state
        .services()
        .packages
        .refresh(&auth.caller(), host.id)
        .await
        .map_err(api_error)?;
    Ok(Json(dto(host.id, &rows)))
}
