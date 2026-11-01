//! The JSON API under `/api/v1`, plus `/healthz` and `/metrics` (ARCHITECTURE.md §10).
//!
//! Handlers are annotated for OpenAPI and registered through [`utoipa_axum`], so the
//! document in `web/src/api/openapi.json` comes from the same route table the hub serves.
//! Every endpoint except login, setup, health and metrics needs a session; unsafe methods
//! also pass the CSRF guard in [`crate::auth::csrf`].

pub mod dto;
pub mod dto_admin;
pub mod error;

mod actions;
mod auth;
mod events;
mod hosts;
mod incidents;
mod notes;
mod settings;
mod system;

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use domovoi_config::HubConfig;
use domovoi_core::ports::{
    ActionRepo, AdvisoryRepo, Clock, FactRepo, HostRepo, IncidentRepo, NoteRepo, Notifier, Page,
    Presence, SignalRepo, UsageRepo,
};
use domovoi_core::{HostId, HostName, SkillSummary};
use domovoi_engine::triage::TriageHandle;
use domovoi_engine::{ActionHandle, DomainEvent, ManagerHandle};
use tokio::sync::broadcast;
use utoipa::OpenApi;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use self::error::ApiError;
use crate::agent_server::{AgentServer, AgentServerError, EnrollmentInvite};
use crate::auth::{AuthState, Current, csrf};

/// The default page size of list endpoints.
pub const DEFAULT_PAGE: u16 = 50;

/// The largest page size of list endpoints.
pub const MAX_PAGE: u16 = 200;

/// Agent administration the API needs: enrollment, revocation, presence and catalogues.
#[async_trait]
pub trait AgentAdmin: Send + Sync {
    /// A one-time enrollment invite for host `name`, creating the host if needed.
    async fn create_invite(&self, name: &HostName) -> Result<EnrollmentInvite, AgentServerError>;
    /// Revokes the live agent of `host`; `false` when it had none.
    async fn revoke(&self, host: HostId) -> Result<bool, AgentServerError>;
    /// Whether `host`'s agent is connected.
    fn presence(&self, host: HostId) -> Presence;
    /// The skills `host`'s agent reported.
    fn skills(&self, host: HostId) -> Vec<SkillSummary>;
}

#[async_trait]
impl AgentAdmin for AgentServer {
    async fn create_invite(&self, name: &HostName) -> Result<EnrollmentInvite, AgentServerError> {
        self.create_token(name).await
    }

    async fn revoke(&self, host: HostId) -> Result<bool, AgentServerError> {
        self.revoke_agent(host).await
    }

    fn presence(&self, host: HostId) -> Presence {
        domovoi_core::ports::AgentLink::presence(&*self.link(), host)
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        domovoi_core::ports::AgentLink::skills(&*self.link(), host)
    }
}

/// A callback rendering Prometheus text for `/metrics`.
pub type MetricsRender = Arc<dyn Fn() -> String + Send + Sync>;

/// Everything the API reads and drives.
pub struct ApiDeps {
    /// Incidents and timelines, read only; changes go through `manager`.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Recent signals, for cards.
    pub signals: Arc<dyn SignalRepo>,
    /// Actions, read only; changes go through `actions_service`.
    pub actions: Arc<dyn ActionRepo>,
    /// Hosts.
    pub hosts: Arc<dyn HostRepo>,
    /// Notes.
    pub notes: Arc<dyn NoteRepo>,
    /// Advisories.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Digest facts.
    pub facts: Arc<dyn FactRepo>,
    /// Model usage for the settings page.
    pub usage: Arc<dyn UsageRepo>,
    /// The incident manager.
    pub manager: ManagerHandle,
    /// Approvals.
    pub actions_service: ActionHandle,
    /// Follow-up questions; `None` when triage is off.
    pub triage: Option<TriageHandle>,
    /// Enrollment, revocation, presence and catalogues.
    pub agents: Arc<dyn AgentAdmin>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Events for the SSE stream.
    pub bus: broadcast::Sender<DomainEvent>,
    /// The loaded configuration, for the settings page and the digest.
    pub config: Arc<HubConfig>,
    /// Where `POST /digest/send` sends; `None` answers 503.
    pub notifier: Option<Arc<dyn Notifier>>,
    /// Renders `/metrics`; `None` serves an empty page.
    pub metrics: Option<MetricsRender>,
}

/// The state every handler gets.
#[derive(Clone)]
pub struct ApiState {
    pub(crate) deps: Arc<ApiDeps>,
    pub(crate) auth: Arc<AuthState>,
}

impl std::fmt::Debug for ApiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiState").finish_non_exhaustive()
    }
}

impl ApiState {
    /// State for the API over `deps`, authenticating with `auth`.
    #[must_use]
    pub fn new(deps: ApiDeps, auth: Arc<AuthState>) -> Self {
        Self {
            deps: Arc::new(deps),
            auth,
        }
    }

    /// The auth state, for the CSRF guard and the setup link.
    #[must_use]
    pub fn auth(&self) -> &Arc<AuthState> {
        &self.auth
    }
}

impl FromRef<ApiState> for Arc<AuthState> {
    fn from_ref(state: &ApiState) -> Self {
        state.auth.clone()
    }
}

/// The logged-in session of a request; 401 without one.
#[derive(Debug, Clone)]
pub struct Authed(pub Current);

impl FromRequestParts<ApiState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        let id = csrf::session_id(&parts.headers).ok_or(ApiError::Unauthorized)?;
        match state.auth.current(id).await? {
            Some(current) => Ok(Self(current)),
            None => Err(ApiError::Unauthorized),
        }
    }
}

/// A page request from optional query parameters.
pub(crate) fn page(cursor: Option<String>, limit: Option<u16>) -> Page {
    Page {
        cursor,
        limit: limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE),
    }
}

/// The host named `name`, or 404.
pub(crate) async fn host_named(
    state: &ApiState,
    name: &str,
) -> Result<domovoi_core::Host, ApiError> {
    let name = HostName::parse(name).map_err(|_| ApiError::NotFound)?;
    state
        .deps
        .hosts
        .get_host_by_name(&name)
        .await?
        .ok_or(ApiError::NotFound)
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Domovoi hub API",
        description = "The hub's JSON API. Log in with `POST /api/v1/auth/login`; the session is a cookie. Send the session's `csrf_token` as `X-CSRF-Token` on every unsafe request."
    ),
    modifiers(&SessionCookie),
    tags(
        (name = "auth", description = "Setup, login and sessions"),
        (name = "incidents", description = "Incidents and their triage"),
        (name = "actions", description = "Proposed actions and approvals"),
        (name = "hosts", description = "Hosts, enrollment and skills"),
        (name = "notes", description = "Host and global notes"),
        (name = "system", description = "Settings, advisories, digest, events and health")
    )
)]
struct ApiDoc;

struct SessionCookie;

impl utoipa::Modify for SessionCookie {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(
                crate::auth::session::COOKIE,
            ))),
        );
    }
}

/// Every route with its OpenAPI description.
#[must_use]
pub fn routes() -> OpenApiRouter<ApiState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(auth::session))
        .routes(routes!(auth::login))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::setup))
        .routes(routes!(incidents::list))
        .routes(routes!(incidents::detail))
        .routes(routes!(incidents::ack))
        .routes(routes!(incidents::mute))
        .routes(routes!(incidents::unmute))
        .routes(routes!(incidents::ask))
        .routes(routes!(actions::list))
        .routes(routes!(actions::approve))
        .routes(routes!(actions::deny))
        .routes(routes!(hosts::list))
        .routes(routes!(hosts::enrollment))
        .routes(routes!(hosts::revoke))
        .routes(routes!(hosts::skills))
        .routes(routes!(notes::list, notes::create))
        .routes(routes!(notes::search))
        .routes(routes!(notes::get, notes::replace, notes::delete))
        .routes(routes!(settings::show))
        .routes(routes!(system::advisories))
        .routes(routes!(system::digest_preview))
        .routes(routes!(system::digest_send))
        .routes(routes!(events::stream))
        .routes(routes!(system::healthz))
        .routes(routes!(system::metrics))
}

/// The OpenAPI document of [`routes()`].
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().split_for_parts().1
}
