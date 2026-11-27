//! What a host reports installed, and asking its agent again.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::{HostId, HostPackage};

use super::dto_packages::{
    HostPackageDto, HostPackagesDto, PackageSkillsDto, PackageSkillsRequestDto,
};
use super::error::{ApiError, ErrorBody};
use super::packages_error::api_error;
use super::{ApiState, Authed, host_named};
use crate::auth::proxy;
use crate::services::StepUp;

fn dto(state: &ApiState, host: HostId, rows: &[HostPackage]) -> HostPackagesDto {
    let states = state.deps.agents.package_skills(host);
    HostPackagesDto {
        host_id: host.to_string(),
        observed_at: rows
            .iter()
            .map(|row| row.observed_at)
            .max()
            .map(|at| at.to_string()),
        entries: rows
            .iter()
            .map(|row| HostPackageDto::from(row).with_skill_states(&states))
            .collect(),
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
    Ok(Json(dto(&state, host.id, &rows)))
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
    Ok(Json(dto(&state, host.id, &rows)))
}

/// Lets the skills of a package stored on host `name` run, or stops them. Asks for the
/// password again and for the package name typed out. The agent checks the files again
/// before it agrees, and the decision is audited as `package.enabled` or `package.disabled`
/// with the plan hash. Installing never enables anything.
#[utoipa::path(
    put, path = "/api/v1/hosts/{name}/packages/{package}/skills", operation_id = "set_package_skills", tag = "packages",
    params(
        ("name" = String, Path, description = "Host name"),
        ("package" = String, Path, description = "Package name"),
    ),
    request_body = PackageSkillsRequestDto,
    responses(
        (status = 200, description = "The agent's answer", body = PackageSkillsDto),
        (status = 400, description = "An unreadable package name or version", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "The password is missing or wrong, or the typed name is not the package's", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 409, description = "Packages are disabled, the agent is not connected, or the agent refused (for example its files changed since install)", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn set_skills(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path((name, package)): Path<(String, String)>,
    Json(req): Json<PackageSkillsRequestDto>,
) -> Result<Json<PackageSkillsDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let package = PackageName::parse(&package)
        .map_err(|_| ApiError::BadRequest("the package name is not valid".to_owned()))?;
    let version = PackageVersion::parse(&req.version)
        .map_err(|_| ApiError::BadRequest("the package version is not valid".to_owned()))?;
    let proof = StepUp {
        ip: proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies),
        password: req.password.as_deref(),
    };
    let answer = state
        .services()
        .packages
        .set_skills_enabled(
            &auth.caller(),
            host.id,
            &package,
            &version,
            req.enabled,
            &req.confirm_text,
            proof,
        )
        .await
        .map_err(api_error)?;
    Ok(Json(PackageSkillsDto {
        name: package.to_string(),
        version: version.to_string(),
        enabled: answer.enabled,
        changed: answer.changed,
        plan_hash: answer.plan_hash.to_string(),
    }))
}
