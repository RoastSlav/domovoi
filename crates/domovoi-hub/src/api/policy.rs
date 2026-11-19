//! The policy editor: the overview, published versions, the draft, previews and publication.
//!
//! Handlers only authenticate, parse and map. Commands go through `services.policy`; plain
//! reads go to the repository. Publishing, and a draft change that touches a locked
//! prohibition, carry the admin's password in the body (step-up), checked against the client
//! address after trusted-proxy handling, so it shares the login limiter.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use domovoi_core::policy_store::PolicyVersionRecord;
use domovoi_core::ports::Page;
use domovoi_engine::policy::Rule;
use domovoi_engine::policy_shadow::decode_rules;
use jiff::SignedDuration;
use serde::Deserialize;
use utoipa::IntoParams;

use super::dto_policy::{
    AckDto, DraftDetailDto, DraftSavedDto, LockChangeDto, PreviewDto, PreviewRequest,
    PublishRequest, PublishedDto, SaveDraftRequest, VersionDetailDto, VersionPageDto,
    VersionSummaryDto,
};
use super::dto_policy_rule::rules_of;
use super::dto_policy_trace::{PolicyOverviewDto, ShadowStateDto};
use super::error::{ApiError, ErrorBody, IssueDto};
use super::{ApiState, Authed};
use crate::auth::proxy;
use crate::services::{PolicyCommandError, RuleIssue, StepUp};

/// The zone `time_window` conditions are evaluated in: the evaluator reads the UTC wall clock.
pub(super) const TIME_WINDOW_ZONE: &str = "UTC";

/// The 400 for rules that do not convert or validate.
pub(super) fn invalid(issues: &[RuleIssue]) -> ApiError {
    ApiError::Invalid(
        issues
            .iter()
            .map(|issue| IssueDto {
                path: issue
                    .rule
                    .as_ref()
                    .map_or_else(String::new, |rule| format!("rules.{rule}")),
                message: issue.message.clone(),
                severity: "error".to_owned(),
            })
            .collect(),
    )
}

/// What a refused policy command means for the caller.
pub(super) fn command_error(err: PolicyCommandError) -> ApiError {
    match err {
        PolicyCommandError::PasswordRequired | PolicyCommandError::BadPassword => {
            ApiError::Forbidden(err.to_string())
        }
        PolicyCommandError::TooMany { retry_after_secs } => ApiError::TooMany(retry_after_secs),
        PolicyCommandError::Invalid(issues) => invalid(&issues),
        PolicyCommandError::NoteTooLong { ref rule }
        | PolicyCommandError::PackageContract { ref rule } => invalid(&[RuleIssue {
            rule: Some(rule.clone()),
            message: err.to_string(),
        }]),
        PolicyCommandError::TooManyRules { .. }
        | PolicyCommandError::TooLarge
        | PolicyCommandError::UnknownAcknowledgement { .. } => {
            ApiError::BadRequest(err.to_string())
        }
        PolicyCommandError::NoDraft | PolicyCommandError::NotFound => ApiError::NotFound,
        PolicyCommandError::DraftExists
        | PolicyCommandError::StaleDraft { .. }
        | PolicyCommandError::StaleActive { .. }
        | PolicyCommandError::StalePreview
        | PolicyCommandError::Unacknowledged { .. } => ApiError::Conflict(err.to_string()),
        PolicyCommandError::Store(err) => ApiError::from(err),
        PolicyCommandError::Commit(err) => ApiError::internal("policy commit failed", err),
    }
}

/// The rules stored for `record`, read back as the editor's rules.
async fn rules_for(state: &ApiState, record: &PolicyVersionRecord) -> Result<Vec<Rule>, ApiError> {
    let stored = state.deps.policy.rules(record.id).await?;
    decode_rules(&stored).map_err(|err| ApiError::internal("stored policy rules unreadable", err))
}

async fn draft_detail(
    state: &ApiState,
    record: &PolicyVersionRecord,
) -> Result<DraftDetailDto, ApiError> {
    Ok(DraftDetailDto::new(
        record,
        &rules_for(state, record).await?,
    ))
}

/// The policy at a glance: the active version, the draft and the shadow recorder.
#[utoipa::path(
    get, path = "/api/v1/policy", operation_id = "policy_overview", tag = "policy",
    responses(
        (status = 200, description = "The overview", body = PolicyOverviewDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn overview(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<PolicyOverviewDto>, ApiError> {
    let repo = &state.deps.policy;
    let now = state.deps.clock.now();
    let since = now
        .checked_sub(SignedDuration::from_hours(24))
        .unwrap_or(now);
    let (active, draft) = (repo.active().await?, repo.draft().await?);
    let shadow = ShadowStateDto::new(
        &repo.shadow_state().await?,
        &repo.shadow_summary(since).await?,
    );
    let policy = &state.services().policy;
    Ok(Json(PolicyOverviewDto {
        shadow_enabled: policy.shadow_enabled(),
        time_window_zone: TIME_WINDOW_ZONE.to_owned(),
        active: active.as_ref().map(VersionSummaryDto::from),
        draft: draft.as_ref().map(Into::into),
        shadow,
        notes: policy
            .notes()
            .await
            .iter()
            .map(|n| (*n).to_owned())
            .collect(),
    }))
}

/// Paging of the version list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct VersionsQuery {
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 100. Default 25.
    pub limit: Option<u16>,
}

/// Published versions, newest first.
#[utoipa::path(
    get, path = "/api/v1/policy/versions", operation_id = "policy_versions", tag = "policy",
    params(VersionsQuery),
    responses(
        (status = 200, description = "One page", body = VersionPageDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn versions(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<VersionsQuery>,
) -> Result<Json<VersionPageDto>, ApiError> {
    let page = Page {
        cursor: query.cursor,
        limit: query.limit.unwrap_or(25).clamp(1, 100),
    };
    let found = state.deps.policy.versions(&page).await?;
    Ok(Json(VersionPageDto {
        items: found.items.iter().map(VersionSummaryDto::from).collect(),
        next_cursor: found.next_cursor,
    }))
}

/// One published version with its rules and acknowledgements.
#[utoipa::path(
    get, path = "/api/v1/policy/versions/{number}", operation_id = "policy_version", tag = "policy",
    params(("number" = u32, Path, description = "The version number")),
    responses(
        (status = 200, description = "The version", body = VersionDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such version", body = ErrorBody),
    )
)]
pub(super) async fn version(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(number): Path<u32>,
) -> Result<Json<VersionDetailDto>, ApiError> {
    let record = state
        .deps
        .policy
        .version(number)
        .await?
        .ok_or(ApiError::NotFound)?;
    let rules = rules_for(&state, &record).await?;
    let acknowledgements = state.deps.policy.acknowledgements(record.id).await?;
    Ok(Json(VersionDetailDto {
        summary: VersionSummaryDto::from(&record),
        rules: rules.iter().map(Into::into).collect(),
        acknowledgements: acknowledgements.iter().map(AckDto::from).collect(),
    }))
}

/// The draft with its rules.
#[utoipa::path(
    get, path = "/api/v1/policy/draft", operation_id = "policy_draft", tag = "policy",
    responses(
        (status = 200, description = "The draft", body = DraftDetailDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "There is no draft", body = ErrorBody),
    )
)]
pub(super) async fn draft(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<DraftDetailDto>, ApiError> {
    let record = state.deps.policy.draft().await?.ok_or(ApiError::NotFound)?;
    Ok(Json(draft_detail(&state, &record).await?))
}

/// Creates the draft or replaces it. Changing a locked prohibition needs the password and the
/// preview hash of exactly these rules.
#[utoipa::path(
    put, path = "/api/v1/policy/draft", operation_id = "policy_save_draft", tag = "policy",
    request_body = SaveDraftRequest,
    responses(
        (status = 200, description = "Saved", body = DraftSavedDto),
        (status = 400, description = "The rules are not valid; `issues` lists each problem", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong for a locked change", body = ErrorBody),
        (status = 409, description = "Stale draft revision, a draft exists, or a stale preview hash", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn save_draft(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Json(req): Json<SaveDraftRequest>,
) -> Result<Json<DraftSavedDto>, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let rules = rules_of(req.rules).map_err(|issues| invalid(&issues))?;
    let saved = state
        .services()
        .policy
        .save_draft(
            &auth.caller(),
            rules,
            req.base_revision,
            req.preview_hash.as_deref(),
            StepUp {
                ip,
                password: req.password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok(Json(DraftSavedDto {
        draft: draft_detail(&state, &saved.record).await?,
        locked_changes: saved
            .locked_changes
            .iter()
            .map(LockChangeDto::from)
            .collect(),
    }))
}

/// Which draft revision to discard.
#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RevisionQuery {
    /// The revision the client read.
    pub revision: u64,
}

/// Discards the draft. It changes nothing live, so it needs no password.
#[utoipa::path(
    delete, path = "/api/v1/policy/draft", operation_id = "policy_discard_draft", tag = "policy",
    params(RevisionQuery),
    responses(
        (status = 204, description = "Discarded"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "There is no draft", body = ErrorBody),
        (status = 409, description = "The draft is at another revision", body = ErrorBody),
    )
)]
pub(super) async fn discard_draft(
    State(state): State<ApiState>,
    auth: Authed,
    Query(query): Query<RevisionQuery>,
) -> Result<StatusCode, ApiError> {
    state
        .services()
        .policy
        .discard_draft(&auth.caller(), query.revision)
        .await
        .map_err(command_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a candidate rule set would do: validation, the publication review, locked changes and
/// a bounded simulation over recent recorded decisions. It writes nothing.
#[utoipa::path(
    post, path = "/api/v1/policy/preview", operation_id = "policy_preview", tag = "policy",
    request_body = PreviewRequest,
    responses(
        (status = 200, description = "The preview", body = PreviewDto),
        (status = 400, description = "A rule does not convert", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn preview(
    State(state): State<ApiState>,
    _auth: Authed,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<PreviewDto>, ApiError> {
    let rules = rules_of(req.rules).map_err(|issues| invalid(&issues))?;
    let preview = state
        .services()
        .policy
        .preview(rules, req.base_revision)
        .await
        .map_err(command_error)?;
    Ok(Json(PreviewDto::from(&preview)))
}

/// Publishes the draft as the next version. Always needs the password; the acknowledged keys
/// must be exactly the blocking findings, and the preview hash that of the draft's rules.
#[utoipa::path(
    post, path = "/api/v1/policy/draft/publish", operation_id = "policy_publish", tag = "policy",
    request_body = PublishRequest,
    responses(
        (status = 200, description = "Published", body = PublishedDto),
        (status = 400, description = "Invalid rules, or an acknowledgement that is not a blocking finding", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 403, description = "Password missing or wrong", body = ErrorBody),
        (status = 404, description = "There is no draft", body = ErrorBody),
        (status = 409, description = "Stale draft, stale active version, stale preview or a missing acknowledgement", body = ErrorBody),
        (status = 429, description = "Too many wrong passwords", body = ErrorBody),
    )
)]
pub(super) async fn publish(
    State(state): State<ApiState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    auth: Authed,
    Json(req): Json<PublishRequest>,
) -> Result<Json<PublishedDto>, ApiError> {
    let ip = proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies);
    let published = state
        .services()
        .policy
        .publish(
            &auth.caller(),
            req.revision,
            &req.preview_hash,
            &req.acknowledged,
            StepUp {
                ip,
                password: req.password.as_deref(),
            },
        )
        .await
        .map_err(command_error)?;
    Ok(Json(PublishedDto {
        version: VersionSummaryDto::from(&published.record),
        locked_changes: published
            .locked_changes
            .iter()
            .map(LockChangeDto::from)
            .collect(),
        acknowledged: published.acknowledged,
    }))
}
