//! Install and uninstall jobs: planning, creating, listing, reading, retrying and cancelling.
//!
//! Creating and retrying ask for the password and the typed package name; the checks and the
//! approval are made by the package service, never here.

use std::net::SocketAddr;
use std::str::FromStr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::{HostId, InstallJobId, InstallKind, PlanHash};

use super::dto_packages::{
    CreateJobRequestDto, InstallPlanDto, JobsQueryDto, PlanRequestDto, RetryRequestDto,
};
use super::dto_packages_jobs::{InstallJobDto, JobPageDto};
use super::error::{ApiError, ErrorBody};
use super::packages_error::api_error;
use super::{ApiState, Authed, page};
use crate::auth::proxy;
use crate::services::{JobInput, JobsQuery, PlanInput, RetryInput, StepUp};

fn bad(message: &str) -> ApiError {
    ApiError::BadRequest(message.to_owned())
}

fn no_such_job() -> ApiError {
    ApiError::Coded {
        status: StatusCode::NOT_FOUND,
        code: "no_such_job",
        message: "no such install job".to_owned(),
    }
}

fn parse_kind(text: &str) -> Result<InstallKind, ApiError> {
    InstallKind::parse(text).ok_or_else(|| bad("kind must be install or uninstall"))
}

fn parse_hosts(ids: &[String]) -> Result<Vec<HostId>, ApiError> {
    ids.iter()
        .map(|id| HostId::from_str(id).map_err(|_| bad("a host id is not valid")))
        .collect()
}

fn parse_hash(text: &str) -> Result<PlanHash, ApiError> {
    text.parse::<PlanHash>()
        .map_err(|_| bad("plan_hash is not a plan hash (64 lowercase hex characters)"))
}

fn parse_name(text: &str) -> Result<PackageName, ApiError> {
    PackageName::parse(text).map_err(|_| bad("the package name is not valid"))
}

fn plan_input(
    kind: &str,
    name: &str,
    version: &str,
    host_ids: &[String],
) -> Result<PlanInput, ApiError> {
    Ok(PlanInput {
        kind: parse_kind(kind)?,
        name: parse_name(name)?,
        version: PackageVersion::parse(version)
            .map_err(|_| bad("the package version is not valid"))?,
        hosts: parse_hosts(host_ids)?,
    })
}

fn job_id(text: &str) -> Result<InstallJobId, ApiError> {
    InstallJobId::from_str(text).map_err(|_| no_such_job())
}

fn host_id(text: &str) -> Result<HostId, ApiError> {
    HostId::from_str(text).map_err(|_| ApiError::NotFound)
}

/// What a job would do on each host. Reads only; nothing is created.
#[utoipa::path(
    post, path = "/api/v1/packages/jobs/plan", operation_id = "plan_package_job", tag = "packages",
    request_body = PlanRequestDto,
    responses(
        (status = 200, description = "What the job would do", body = InstallPlanDto),
        (status = 400, description = "Not one to sixteen distinct hosts, or an unreadable name", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such package or host", body = ErrorBody),
    )
)]
pub(super) async fn plan(
    State(state): State<ApiState>,
    _auth: Authed,
    Json(req): Json<PlanRequestDto>,
) -> Result<Json<InstallPlanDto>, ApiError> {
    let input = plan_input(&req.kind, &req.name, &req.version, &req.host_ids)?;
    let plan = state
        .services()
        .packages
        .plan(&input)
        .await
        .map_err(api_error)?;
    Ok(Json(InstallPlanDto::from(&plan)))
}

/// Creates an install or uninstall job with one target per host. Asks for the password
/// again and for the package name typed out, and refuses a plan that changed since the owner
/// looked at it. A job is approved here and nowhere else.
#[utoipa::path(
    post, path = "/api/v1/packages/jobs", operation_id = "create_package_job", tag = "packages",
    request_body = CreateJobRequestDto,
    responses(
        (status = 202, description = "Created; the work runs in the background", body = InstallJobDto),
        (status = 400, description = "Not one to sixteen distinct hosts, or an unreadable name or hash", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "The password is missing or wrong, or the typed name is not the package's", body = ErrorBody),
        (status = 404, description = "No such package or host", body = ErrorBody),
        (status = 409, description = "Packages are disabled, the plan changed, or a host already has a job in progress", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn create(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Json(req): Json<CreateJobRequestDto>,
) -> Result<(StatusCode, Json<InstallJobDto>), ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let input = JobInput {
        plan: plan_input(&req.kind, &req.name, &req.version, &req.host_ids)?,
        plan_hash: parse_hash(&req.plan_hash)?,
        confirm_text: req.confirm_text,
    };
    let proof = StepUp {
        ip,
        password: req.password.as_deref(),
    };
    let view = state
        .services()
        .packages
        .create_job(&auth.caller(), input, proof)
        .await
        .map_err(api_error)?;
    Ok((StatusCode::ACCEPTED, Json(InstallJobDto::from(&view))))
}

/// Install jobs, newest first.
#[utoipa::path(
    get, path = "/api/v1/packages/jobs", operation_id = "list_package_jobs", tag = "packages",
    params(JobsQueryDto),
    responses(
        (status = 200, description = "A page of jobs", body = JobPageDto),
        (status = 400, description = "An unreadable filter", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<JobsQueryDto>,
) -> Result<Json<JobPageDto>, ApiError> {
    let active = match query.status.as_deref() {
        None => None,
        Some("active") => Some(true),
        Some("finished") => Some(false),
        Some(_) => return Err(bad("status must be active or finished")),
    };
    let host = query
        .host_id
        .as_deref()
        .map(|text| HostId::from_str(text).map_err(|_| bad("host_id is not valid")))
        .transpose()?;
    let name = query.name.as_deref().map(parse_name).transpose()?;
    let found = state
        .services()
        .packages
        .jobs(JobsQuery {
            host,
            name,
            active,
            page: Some(page(query.cursor, query.limit)),
        })
        .await
        .map_err(api_error)?;
    Ok(Json(JobPageDto {
        items: found.items.iter().map(InstallJobDto::from).collect(),
        next_cursor: found.next_cursor,
    }))
}

/// One job with every attempt on every host.
#[utoipa::path(
    get, path = "/api/v1/packages/jobs/{id}", operation_id = "get_package_job", tag = "packages",
    params(("id" = String, Path, description = "Job id")),
    responses(
        (status = 200, description = "The job", body = InstallJobDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such job", body = ErrorBody),
    )
)]
pub(super) async fn get(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<InstallJobDto>, ApiError> {
    let view = state
        .services()
        .packages
        .job(job_id(&id)?)
        .await
        .map_err(api_error)?;
    Ok(Json(InstallJobDto::from(&view)))
}

/// Runs one host's target again as a new attempt, after a failure, a refusal or an unknown
/// outcome. Asks for the password and the typed package name like a new job.
#[utoipa::path(
    post, path = "/api/v1/packages/jobs/{id}/targets/{host_id}/retry", operation_id = "retry_package_target", tag = "packages",
    params(
        ("id" = String, Path, description = "Job id"),
        ("host_id" = String, Path, description = "Host id"),
    ),
    request_body = RetryRequestDto,
    responses(
        (status = 202, description = "A new attempt was queued", body = InstallJobDto),
        (status = 400, description = "An unreadable hash", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "The password is missing or wrong, or the typed name is not the package's", body = ErrorBody),
        (status = 404, description = "No such job or target", body = ErrorBody),
        (status = 409, description = "Packages are disabled, the plan changed, the latest attempt cannot be retried, or the host is busy", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn retry(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path((id, host)): Path<(String, String)>,
    Json(req): Json<RetryRequestDto>,
) -> Result<(StatusCode, Json<InstallJobDto>), ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let input = RetryInput {
        job: job_id(&id)?,
        host: host_id(&host)?,
        plan_hash: parse_hash(&req.plan_hash)?,
        confirm_text: req.confirm_text,
    };
    let proof = StepUp {
        ip,
        password: req.password.as_deref(),
    };
    let view = state
        .services()
        .packages
        .retry(&auth.caller(), input, proof)
        .await
        .map_err(api_error)?;
    Ok((StatusCode::ACCEPTED, Json(InstallJobDto::from(&view))))
}

/// Cancels a host's target that has not started. Once it has started it cannot be cancelled.
#[utoipa::path(
    post, path = "/api/v1/packages/jobs/{id}/targets/{host_id}/cancel", operation_id = "cancel_package_target", tag = "packages",
    params(
        ("id" = String, Path, description = "Job id"),
        ("host_id" = String, Path, description = "Host id"),
    ),
    responses(
        (status = 200, description = "Cancelled", body = InstallJobDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such job or target", body = ErrorBody),
        (status = 409, description = "Already started; it cannot be cancelled", body = ErrorBody),
    )
)]
pub(super) async fn cancel(
    State(state): State<ApiState>,
    auth: Authed,
    Path((id, host)): Path<(String, String)>,
) -> Result<Json<InstallJobDto>, ApiError> {
    let view = state
        .services()
        .packages
        .cancel(&auth.caller(), job_id(&id)?, host_id(&host)?)
        .await
        .map_err(api_error)?;
    Ok(Json(InstallJobDto::from(&view)))
}
