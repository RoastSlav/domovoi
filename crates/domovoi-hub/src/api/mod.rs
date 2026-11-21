//! The JSON API under `/api/v1`, plus `/healthz` and `/metrics` (ARCHITECTURE.md §10).
//!
//! Handlers are annotated for OpenAPI and registered through [`utoipa_axum`], so the
//! document in `web/src/api/openapi.json` comes from the same route table the hub serves.
//! Every endpoint except login, setup, health and metrics needs a session; unsafe methods
//! also pass the CSRF guard in [`crate::auth::csrf`].

pub mod dto;
pub mod dto_admin;
pub mod dto_experience;
pub mod dto_knowledge;
pub mod dto_operations;
pub mod dto_policy;
pub mod dto_policy_query;
pub mod dto_policy_rule;
pub mod dto_policy_trace;
pub mod dto_query;
pub mod dto_trace;
pub mod error;
pub mod notes_import;

mod actions;
mod auth;
mod capability;
mod events;
mod experience;
mod followups;
mod host_docker;
pub(crate) mod hosts;
pub(crate) mod incidents;
mod knowledge;
mod knowledge_graph;
mod logs;
mod notes;
mod operation_controls;
pub(crate) mod operation_errors;
pub(crate) mod operations;
pub mod operator;
mod overview;
mod plan_binding;
mod policy;
mod policy_shadow;
mod preferences;
pub(crate) mod profile;
pub mod readiness;
mod rules;
pub(crate) mod settings;
mod settings_write;
mod snapshot;
pub(crate) mod system;
mod tool_usage;
mod updates;

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, FromRef, FromRequestParts};
use axum::http::request::Parts;
use domovoi_config::HubConfig;
use domovoi_core::HostName;
use domovoi_core::ports::{
    AckRepo, ActionRepo, AdvisoryRepo, Clock, HostRepo, IncidentRepo, LogSource, MetricSampleRepo,
    NoteRepo, Notifier, Page, PolicyRepo, Redactor, SignalRepo, UsageRepo,
};
use domovoi_engine::ManagerHandle;
use domovoi_engine::triage::TriageHandle;
use tokio_util::sync::CancellationToken;
use utoipa::OpenApi;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub use crate::agent_server::AgentAdmin;

use self::error::ApiError;
use crate::auth::{AuthState, Current, csrf};
use crate::services::{Caller, Services};

/// The default page size of list endpoints.
pub const DEFAULT_PAGE: u16 = 50;

/// The largest page size of list endpoints.
pub const MAX_PAGE: u16 = 200;

/// A callback rendering Prometheus text for `/metrics`.
pub type MetricsRender = Arc<dyn Fn() -> String + Send + Sync>;

/// Everything the API reads and drives.
#[derive(Clone)]
pub struct ApiDeps {
    /// Incidents and timelines, read only; changes go through `manager`.
    pub incidents: Arc<dyn IncidentRepo>,
    /// Recent signals, for cards.
    pub signals: Arc<dyn SignalRepo>,
    /// Actions, read only: retired legacy history.
    pub actions: Arc<dyn ActionRepo>,
    /// Hosts.
    pub hosts: Arc<dyn HostRepo>,
    /// Notes.
    pub notes: Arc<dyn NoteRepo>,
    /// Advisories.
    pub advisories: Arc<dyn AdvisoryRepo>,
    /// Digest facts.
    pub facts: Arc<dyn MetricSampleRepo>,
    /// Model usage for the settings page.
    pub usage: Arc<dyn UsageRepo>,
    /// Ack rules ("known conditions"), read for the list and written by the UI.
    pub acks: Arc<dyn AckRepo>,
    /// The incident manager.
    pub manager: ManagerHandle,
    /// The use-case layer every command goes through.
    pub services: Services,
    /// Follow-up questions; `None` when triage is off.
    pub triage: Option<TriageHandle>,
    /// Stored operations, read only; changes go through `services.operations`.
    pub operation_repo: Arc<dyn domovoi_core::ports::OperationRepo>,
    /// Resolution summaries of finished operations, read only; edits go through
    /// `services.operations`.
    pub experience: Arc<dyn domovoi_core::ports::ExperienceRead>,
    /// Policy versions and recorded shadow decisions, read only; changes go through
    /// `services.policy`.
    pub policy: Arc<dyn PolicyRepo>,
    /// Enrollment, revocation, presence and catalogues.
    pub agents: Arc<dyn AgentAdmin>,
    /// The capability registry: the per-host skill switches the UI reads and writes.
    pub capabilities: Arc<crate::capability::HubRegistry>,
    /// The profile task's handles; `None` when the hub runs without one, where the
    /// profile endpoints answer 503.
    pub profiles: Option<crate::profile::ProfileDeps>,
    /// Operator conversations and their messages, read only; changes go through
    /// `services.conversations`.
    pub conversations: Arc<dyn domovoi_core::ports::ConversationRepo>,
    /// Where live events of answers being written are published and read by event streams.
    pub live: Arc<operator::live::LiveBus>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// The journal the event stream and the snapshot positions read.
    pub journal: Arc<dyn domovoi_core::ports::JournalFeed>,
    /// The loaded configuration, for the settings page and the digest.
    pub config: Arc<HubConfig>,
    /// Where `POST /digest/send` sends; `None` answers 503.
    pub notifier: Option<Arc<dyn Notifier>>,
    /// Renders `/metrics`; `None` serves an empty page.
    pub metrics: Option<MetricsRender>,
    /// What `/readyz` asks the store.
    pub store_health: Arc<dyn domovoi_core::ports::StoreHealth>,
    /// Settings managed from the UI; `None` when the hub was built from a fixed
    /// configuration, where the settings page is read-only.
    pub settings: Option<crate::settings::SettingsService>,
    /// The audit log.
    pub audit: Arc<dyn domovoi_core::ports::AuditLog>,
    /// Recent log lines of this process; `None` answers 503 where logs were not captured.
    pub logs: Option<Arc<dyn LogSource>>,
    /// Removes secrets from log text before it is served.
    pub redactor: Arc<dyn Redactor>,
    /// The last release check, and the button that runs one now.
    pub update: crate::runtime::updates::UpdateChecker,
}

/// The state every handler gets.
#[derive(Clone)]
pub struct ApiState {
    pub(crate) deps: Arc<ApiDeps>,
    pub(crate) auth: Arc<AuthState>,
    /// Ends long-lived responses (the event stream) when the server shuts down.
    pub(crate) shutdown: CancellationToken,
    /// How many event streams are open, against the cap.
    pub(crate) event_streams: Arc<std::sync::atomic::AtomicUsize>,
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
            shutdown: CancellationToken::new(),
            event_streams: Arc::default(),
        }
    }

    /// The same state, with event streams ending when `shutdown` is cancelled so a graceful
    /// shutdown does not wait on open browser tabs.
    #[must_use]
    pub fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = shutdown;
        self
    }

    /// The repositories and handles the API works with, shared with the chat authorizer.
    #[must_use]
    pub fn deps(&self) -> &Arc<ApiDeps> {
        &self.deps
    }

    /// The use-case layer.
    #[must_use]
    pub fn services(&self) -> &Services {
        &self.deps.services
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

/// The logged-in session of a request; 401 without one, with `setup_required` while no
/// account exists.
#[derive(Debug, Clone)]
pub struct Authed(pub Current);

impl Authed {
    /// The caller services act for: the logged-in person on the web surface. The only place
    /// a web [`Caller`] is built.
    #[must_use]
    pub fn caller(&self) -> Caller {
        Caller::web(self.0.user.id, self.0.session_hash)
    }
}

impl FromRequestParts<ApiState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, ApiError> {
        if let Some(id) = csrf::session_id(&parts.headers)
            && let Some(current) = state.auth.current(id).await?
        {
            return Ok(Self(current));
        }
        if state.auth.setup_required().await? {
            return Err(ApiError::SetupRequired);
        }
        Err(ApiError::Unauthorized)
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
        (name = "actions", description = "Retired legacy actions, read only"),
        (name = "operations", description = "Operations, their steps and step approvals"),
        (name = "experience", description = "Summaries of finished operations, similar past operations and search"),
        (name = "policy", description = "Policy versions, shadow decisions, the simulator and publication"),
        (name = "knowledge", description = "Facts, relations and entities the hub knows"),
        (name = "operator", description = "Operator conversations: read-only questions about the hosts"),
        (name = "hosts", description = "Hosts, enrollment and skills"),
        (name = "notes", description = "Host and global notes"),
        (name = "me", description = "The signed-in person's own preferences"),
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
        .routes(routes!(incidents::dismiss))
        .routes(routes!(incidents::ask))
        .routes(routes!(
            operator::list_conversations,
            operator::create_conversation
        ))
        .routes(routes!(
            operator::get_conversation,
            operator::delete_conversation
        ))
        .routes(routes!(operator::send_message))
        .routes(routes!(operator::cancel_turn))
        .routes(routes!(operator::retry_message))
        .routes(routes!(operator::set_conversation_operation))
        .routes(routes!(followups::history, followups::clear))
        .routes(routes!(incidents::retry_triage))
        .routes(routes!(actions::list))
        .routes(routes!(operations::list))
        .routes(routes!(operations::detail))
        .routes(routes!(operations::trace))
        .routes(routes!(operations::step))
        .routes(routes!(operations::approve_step))
        .routes(routes!(operations::deny_step))
        .routes(routes!(operation_controls::cancel))
        .routes(routes!(operation_controls::pause))
        .routes(routes!(operation_controls::resume))
        .routes(routes!(operation_controls::answer))
        .routes(routes!(experience::get_operation_summary))
        .routes(routes!(experience::edit_operation_summary_field))
        .routes(routes!(experience::get_similar_operations))
        .routes(routes!(experience::get_similar_operations_for_incident))
        .routes(routes!(experience::search_experience_owner))
        .routes(routes!(policy::overview))
        .routes(routes!(policy::versions))
        .routes(routes!(policy::version))
        .routes(routes!(
            policy::draft,
            policy::save_draft,
            policy::discard_draft
        ))
        .routes(routes!(policy::preview))
        .routes(routes!(policy::publish))
        .routes(routes!(policy_shadow::simulate))
        .routes(routes!(policy_shadow::list))
        .routes(routes!(policy_shadow::detail))
        .routes(routes!(knowledge::list, knowledge::assert_fact))
        .routes(routes!(knowledge::get))
        .routes(routes!(knowledge::accept))
        .routes(routes!(knowledge::retract))
        .routes(routes!(knowledge::verify))
        .routes(routes!(knowledge::confirm))
        .routes(routes!(knowledge::management_paths))
        .routes(routes!(
            knowledge_graph::list_relations,
            knowledge_graph::assert_relation
        ))
        .routes(routes!(knowledge_graph::retract_relation))
        .routes(routes!(
            knowledge_graph::list_entities,
            knowledge_graph::create_entity
        ))
        .routes(routes!(
            knowledge_graph::rename_entity,
            knowledge_graph::delete_entity
        ))
        .routes(routes!(hosts::list))
        .routes(routes!(hosts::detail))
        .routes(routes!(hosts::enrollment))
        .routes(routes!(hosts::revoke))
        .routes(routes!(hosts::skills))
        .routes(routes!(capability::list_capabilities))
        .routes(routes!(hosts::logs))
        .routes(routes!(profile::get, profile::refresh))
        .routes(routes!(notes::list, notes::create))
        .routes(routes!(notes::search))
        .routes(routes!(notes::get, notes::replace, notes::delete))
        .routes(routes!(preferences::get, preferences::put))
        .routes(routes!(rules::list, rules::create))
        .routes(routes!(rules::delete))
        .routes(routes!(settings::show, settings_write::update))
        .routes(routes!(settings_write::schema))
        .routes(routes!(settings_write::test_discord))
        .routes(routes!(settings_write::test_llm))
        .routes(routes!(settings_write::list_models))
        .routes(routes!(settings_write::test_heartbeat))
        .routes(routes!(
            settings_write::host_checks,
            settings_write::set_host_checks
        ))
        .routes(routes!(
            host_docker::host_docker,
            host_docker::set_host_docker
        ))
        .routes(routes!(settings_write::set_skill_enabled))
        .routes(routes!(system::advisories))
        .routes(routes!(system::digest_preview))
        .routes(routes!(system::digest_send))
        .routes(routes!(updates::show, updates::check))
        .routes(routes!(overview::show))
        .routes(routes!(events::stream))
        .routes(routes!(system::healthz))
        .routes(routes!(readiness::readyz))
        .routes(routes!(system::metrics))
        .routes(routes!(logs::list))
        .merge(
            // Inside the router's 1 MiB default, so this route alone takes a folder of notes.
            OpenApiRouter::new()
                .routes(routes!(notes_import::import))
                .layer(DefaultBodyLimit::max(notes_import::MAX_REQUEST_BYTES)),
        )
}

/// The OpenAPI document of [`routes()`].
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().split_for_parts().1
}
