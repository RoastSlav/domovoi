//! Knowledge facts and management paths.

use domovoi_core::knowledge::PathState;
use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use domovoi_core::id::FactRecordId;
use domovoi_core::knowledge::{EvidenceRef, FactSource, Predicate, SubjectRef};
use domovoi_core::ports::{FactFilter, Page};
use domovoi_engine::knowledge::{FactDraft, KnowledgeError};

use super::dto_knowledge::{
    ConfirmRefusalDto, FactListQuery, KnowledgeFactAction, KnowledgeFactDto, KnowledgeFactInput,
    KnowledgeFactPageDto, ManagementPathDto, ManagementPathsDto, parse_statuses,
};
use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};
use crate::auth::proxy;
use crate::services::{KNOWLEDGE_PASSWORD_REQUIRED, KnowledgeCommandError, StepUp};

/// The default and largest page of a plain listing.
const DEFAULT_LIMIT: u16 = 50;
const MAX_LIMIT: u16 = 200;
/// The most hits a search returns.
const MAX_SEARCH: u16 = 50;

/// Maps a knowledge use-case failure to the API's error.
pub(super) fn command_error(err: KnowledgeCommandError) -> ApiError {
    match err {
        KnowledgeCommandError::PasswordRequired => {
            ApiError::Forbidden(KNOWLEDGE_PASSWORD_REQUIRED.to_owned())
        }
        KnowledgeCommandError::BadPassword => ApiError::Forbidden("wrong password".to_owned()),
        KnowledgeCommandError::TooMany { retry_after_secs } => ApiError::TooMany(retry_after_secs),
        KnowledgeCommandError::Knowledge(err) => knowledge_error(err),
    }
}

fn knowledge_error(err: KnowledgeError) -> ApiError {
    match err {
        KnowledgeError::InvalidSubject(_)
        | KnowledgeError::InvalidValue(_)
        | KnowledgeError::InvalidInput(_)
        | KnowledgeError::SubjectNotAllowed { .. } => ApiError::BadRequest(err.to_string()),
        KnowledgeError::ActorNotAllowed { .. } => ApiError::Forbidden(err.to_string()),
        KnowledgeError::NotFound => ApiError::NotFound,
        KnowledgeError::Conflict
        | KnowledgeError::NotLive { .. }
        | KnowledgeError::NotOwnerFact
        | KnowledgeError::UseConfirmation
        | KnowledgeError::PathUncovered
        | KnowledgeError::CoverageUnverifiable(_)
        | KnowledgeError::DuplicateEntity
        | KnowledgeError::EntityInUse => ApiError::Conflict(err.to_string()),
        KnowledgeError::Store(err) => ApiError::from(err),
    }
}

/// A fact or relation id from a path, or 404.
pub(super) fn parse_id<T: std::str::FromStr>(id: &str) -> Result<T, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound)
}

pub(super) fn subject(text: &str) -> Result<SubjectRef, ApiError> {
    text.parse()
        .map_err(|err| ApiError::BadRequest(format!("invalid subject: {err}")))
}

pub(super) fn evidence(text: Option<&str>) -> Result<Option<EvidenceRef>, ApiError> {
    text.map(|t| {
        t.parse()
            .map_err(|_| ApiError::BadRequest("the evidence reference is not valid".to_owned()))
    })
    .transpose()
}

/// The limit of a listing, clamped.
pub(super) fn limit_of(limit: Option<u16>, max: u16) -> u16 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, max)
}

/// Lists facts, or searches them when `q` is given.
#[utoipa::path(
    get, path = "/api/v1/knowledge/facts", operation_id = "list_facts", tag = "knowledge",
    params(FactListQuery),
    responses(
        (status = 200, description = "The facts, newest first", body = KnowledgeFactPageDto),
        (status = 400, description = "A filter value is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<FactListQuery>,
) -> Result<Json<KnowledgeFactPageDto>, ApiError> {
    let host = match query.host.as_deref() {
        Some(name) => Some(host_named(&state, name).await?.id),
        None => None,
    };
    let filter = FactFilter {
        host,
        subject: query.subject.as_deref().map(subject).transpose()?,
        predicate: query
            .predicate
            .as_deref()
            .map(|p| {
                Predicate::parse(p)
                    .ok_or_else(|| ApiError::BadRequest("unknown predicate".to_owned()))
            })
            .transpose()?,
        source: query
            .source
            .as_deref()
            .map(|s| {
                FactSource::parse(s)
                    .ok_or_else(|| ApiError::BadRequest("unknown source".to_owned()))
            })
            .transpose()?,
        statuses: query
            .status
            .as_deref()
            .map(|s| {
                parse_statuses(s).ok_or_else(|| ApiError::BadRequest("unknown status".to_owned()))
            })
            .transpose()?,
    };
    let knowledge = &state.services().knowledge;
    let now = state.deps.clock.now();
    let (facts, next_cursor) = match query.q.as_deref() {
        Some(q) => {
            let hits = knowledge
                .search(q, host, limit_of(query.limit, MAX_SEARCH))
                .await
                .map_err(command_error)?;
            let facts = hits
                .into_iter()
                .map(|hit| hit.fact)
                .filter(|f| narrows(&filter, f))
                .collect::<Vec<_>>();
            (facts, None)
        }
        None => {
            let page = Page {
                cursor: query.cursor,
                limit: limit_of(query.limit, MAX_LIMIT),
            };
            let page = knowledge
                .facts(&filter, &page)
                .await
                .map_err(command_error)?;
            (page.items, page.next_cursor)
        }
    };
    let versioned = knowledge
        .with_versions(&facts)
        .await
        .map_err(command_error)?;
    Ok(Json(KnowledgeFactPageDto {
        items: versioned
            .iter()
            .map(|f| KnowledgeFactDto::new(f, now))
            .collect(),
        next_cursor,
    }))
}

/// Whether a searched fact passes the filters other than the text.
fn narrows(filter: &FactFilter, fact: &domovoi_core::knowledge::Fact) -> bool {
    filter.subject.as_ref().is_none_or(|s| s == &fact.subject)
        && filter.predicate.is_none_or(|p| p == fact.predicate)
        && filter.source.is_none_or(|s| s == fact.source)
        && filter
            .statuses
            .as_ref()
            .is_none_or(|list| list.contains(&fact.status))
}

/// One fact.
#[utoipa::path(
    get, path = "/api/v1/knowledge/facts/{id}", operation_id = "get_fact", tag = "knowledge",
    params(("id" = String, Path, description = "The fact id")),
    responses(
        (status = 200, description = "The fact", body = KnowledgeFactDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such fact", body = ErrorBody),
    )
)]
pub(super) async fn get(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeFactDto>, ApiError> {
    let fact = state
        .services()
        .knowledge
        .fact(parse_id(&id)?)
        .await
        .map_err(command_error)?;
    Ok(Json(KnowledgeFactDto::new(&fact, state.deps.clock.now())))
}

/// States a fact as the owner. A safety-critical predicate needs the password.
#[utoipa::path(
    post, path = "/api/v1/knowledge/facts", operation_id = "assert_fact", tag = "knowledge",
    request_body = KnowledgeFactInput,
    responses(
        (status = 201, description = "Recorded", body = KnowledgeFactDto),
        (status = 400, description = "The fact is not valid", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn assert_fact(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Json(input): Json<KnowledgeFactInput>,
) -> Result<(StatusCode, Json<KnowledgeFactDto>), ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let draft = FactDraft {
        subject: subject(&input.subject)?,
        predicate: Predicate::parse(&input.predicate)
            .ok_or_else(|| ApiError::BadRequest("unknown predicate".to_owned()))?,
        value: input.value,
        source: FactSource::User,
        observed_at: None,
        valid_until: input.valid_until,
        evidence: evidence(input.evidence.as_deref())?,
    };
    let recorded = state
        .services()
        .knowledge
        .assert_fact(
            &auth.caller(),
            draft,
            StepUp {
                ip,
                password: input.password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok((
        StatusCode::CREATED,
        Json(KnowledgeFactDto::new(
            &recorded.fact,
            state.deps.clock.now(),
        )),
    ))
}

macro_rules! fact_action {
    ($(#[$meta:meta])* $name:ident, $path:literal, $op:literal, $summary:literal, $call:ident) => {
        $(#[$meta])*
        #[utoipa::path(
            post, path = $path, operation_id = $op, tag = "knowledge",
            params(("id" = String, Path, description = "The fact id")),
            request_body = KnowledgeFactAction,
            responses(
                (status = 200, description = $summary, body = KnowledgeFactDto),
                (status = 401, description = "Not logged in", body = ErrorBody),
                (status = 403, description = "Password missing or wrong", body = ErrorBody),
                (status = 404, description = "No such fact", body = ErrorBody),
                (status = 409, description = "The fact is not live, or someone changed it meanwhile", body = ErrorBody),
                (status = 429, description = "Too many wrong passwords", body = ErrorBody),
            )
        )]
        pub(super) async fn $name(
            State(state): State<ApiState>,
            ConnectInfo(peer): ConnectInfo<SocketAddr>,
            headers: HeaderMap,
            auth: Authed,
            Path(id): Path<String>,
            Json(input): Json<KnowledgeFactAction>,
        ) -> Result<Json<KnowledgeFactDto>, ApiError> {
            let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
            let id: FactRecordId = parse_id(&id)?;
            let proof = StepUp {
                ip,
                password: input.password.as_deref(),
            };
            let fact = state
                .services()
                .knowledge
                .$call(&auth.caller(), id, input.expected_version, proof)
                .await
                .map_err(command_error)?;
            let fact = Stored::from(fact).0;
            Ok(Json(KnowledgeFactDto::new(&fact, state.deps.clock.now())))
        }
    };
}

/// What `accept` and `verify` hand back, as one stored fact.
struct Stored(domovoi_core::ports::Versioned<domovoi_core::knowledge::Fact>);

impl From<domovoi_core::ports::Versioned<domovoi_core::knowledge::Fact>> for Stored {
    fn from(v: domovoi_core::ports::Versioned<domovoi_core::knowledge::Fact>) -> Self {
        Self(v)
    }
}

impl From<domovoi_engine::knowledge::Recorded> for Stored {
    fn from(r: domovoi_engine::knowledge::Recorded) -> Self {
        Self(r.fact)
    }
}

fact_action!(
    /// Makes a fact the owner's own. A safety-critical predicate needs the password.
    accept, "/api/v1/knowledge/facts/{id}/accept", "accept_fact", "Accepted", accept
);
fact_action!(
    /// Says a fact still holds. A safety-critical predicate needs the password.
    verify, "/api/v1/knowledge/facts/{id}/verify", "verify_fact", "Verified", verify
);

/// Withdraws a fact. It only removes trust, so it needs no password.
#[utoipa::path(
    post, path = "/api/v1/knowledge/facts/{id}/retract", operation_id = "retract_fact",
    tag = "knowledge",
    params(("id" = String, Path, description = "The fact id")),
    request_body = KnowledgeFactAction,
    responses(
        (status = 200, description = "Retracted", body = KnowledgeFactDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such fact", body = ErrorBody),
        (status = 409, description = "The fact is not live, or someone changed it meanwhile", body = ErrorBody),
    )
)]
pub(super) async fn retract(
    State(state): State<ApiState>,
    auth: Authed,
    Path(id): Path<String>,
    Json(input): Json<KnowledgeFactAction>,
) -> Result<Json<KnowledgeFactDto>, ApiError> {
    let fact = state
        .services()
        .knowledge
        .retract(&auth.caller(), parse_id(&id)?, input.expected_version)
        .await
        .map_err(command_error)?;
    Ok(Json(KnowledgeFactDto::new(&fact, state.deps.clock.now())))
}

/// Confirms a management path against the helper's verified coverage. Needs the password.
#[utoipa::path(
    post, path = "/api/v1/knowledge/facts/{id}/confirm", operation_id = "confirm_management_path",
    tag = "knowledge",
    params(("id" = String, Path, description = "The fact id")),
    request_body = KnowledgeFactAction,
    responses(
        (status = 200, description = "Confirmed", body = ManagementPathDto),
        (status = 400, description = "The fact is not a management path", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong", body = ErrorBody),
        (status = 404, description = "No such fact", body = ErrorBody),
        (status = 409, description = "Not the owner's, not live, not covered, or coverage cannot be verified", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn confirm(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Path(id): Path<String>,
    Json(input): Json<KnowledgeFactAction>,
) -> Result<Json<ManagementPathDto>, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let view = state
        .services()
        .knowledge
        .confirm(
            &auth.caller(),
            parse_id(&id)?,
            input.expected_version,
            StepUp {
                ip,
                password: input.password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok(Json(ManagementPathDto::new(&view, state.deps.clock.now())))
}

/// A host's management paths.
#[utoipa::path(
    get, path = "/api/v1/knowledge/hosts/{name}/management-paths",
    operation_id = "list_management_paths", tag = "knowledge",
    params(("name" = String, Path, description = "The host name")),
    responses(
        (status = 200, description = "The paths and whether coverage can be verified", body = ManagementPathsDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn management_paths(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<ManagementPathsDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let knowledge = &state.services().knowledge;
    let paths = knowledge
        .management_paths(host.id)
        .await
        .map_err(command_error)?;
    let now = state.deps.clock.now();
    let mut listed = Vec::with_capacity(paths.len());
    for path in &paths {
        let mut dto = ManagementPathDto::new(path, now);
        if path.state != PathState::Confirmed {
            dto.last_refusal = knowledge
                .confirm_refusal(path.fact.value.id)
                .await
                .map_err(command_error)?
                .map(|refusal| ConfirmRefusalDto {
                    at: refusal.at,
                    reason: refusal.reason,
                });
        }
        listed.push(dto);
    }
    Ok(Json(ManagementPathsDto {
        host: host.name.to_string(),
        coverage_available: knowledge.coverage_available(),
        paths: listed,
    }))
}
