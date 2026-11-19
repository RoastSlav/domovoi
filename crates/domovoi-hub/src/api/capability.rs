//! The read-only capability inventory of one host: what the hub would offer here, and why not.
//!
//! Every row is computed by the capability registry; this module only projects it. It answers
//! no policy question, offers no approval and writes nothing: an `available` row is exactly
//! "the host implements it, the hub enables it and nothing it needs is missing".

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::capability::{AvailabilityEntry, CatalogueState};
use serde::Serialize;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};

/// The catalogue state of one host, as far as the hub has read it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogueDto {
    /// `complete`, `partial` or `stale`.
    pub state: String,
    /// The gap code when the list is not verified.
    pub gap: Option<String>,
    /// The hash the entries were walked under, when there is one.
    pub hash: Option<String>,
}

impl From<&CatalogueState> for CatalogueDto {
    fn from(state: &CatalogueState) -> Self {
        match state {
            CatalogueState::Complete { hash } => Self {
                state: "complete".to_owned(),
                gap: None,
                hash: Some(hash.clone()),
            },
            CatalogueState::Partial { gap } => Self {
                state: "partial".to_owned(),
                gap: Some(gap.code().to_owned()),
                hash: None,
            },
            CatalogueState::Stale { snapshot, .. } => Self {
                state: "stale".to_owned(),
                gap: None,
                hash: Some(snapshot.clone()),
            },
        }
    }
}

/// One capability as this host sees it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CapabilityDto {
    /// Contract id, `<family>.<verb>`.
    pub contract: String,
    /// Contract version.
    pub version: u32,
    /// Short title for the inventory.
    pub title: String,
    /// Coarse family used for policy specificity.
    pub category: String,
    /// The effect class the contract carries.
    pub effect: String,
    /// What kind of thing it acts on.
    pub target_kind: String,
    /// Where it executes.
    pub execution: String,
    /// The availability state code: one of the eight.
    pub state: String,
    /// The reason code of a refusal, when it has one.
    pub reason: Option<String>,
    /// A short owner-facing sentence about the state.
    pub describe: String,
    /// How much of the host's implementation list the hub has read.
    pub catalogue: CatalogueDto,
    /// Whether the agent runs as root. A label, never a refusal.
    pub root_agent: bool,
    /// Whether the agent reported a working isolated skill runner.
    pub runner_installed: bool,
    /// Whether the implementation came from `hello.skills[]`.
    pub from_hello_skills: bool,
}

impl From<AvailabilityEntry> for CapabilityDto {
    fn from(entry: AvailabilityEntry) -> Self {
        Self {
            contract: entry.contract.to_string(),
            version: entry.version.get(),
            title: entry.title,
            category: entry.category.as_str().to_owned(),
            effect: format!("{:?}", entry.effect).to_lowercase(),
            target_kind: format!("{:?}", entry.target_kind).to_lowercase(),
            execution: format!("{:?}", entry.execution).to_lowercase(),
            state: entry.state.code().to_owned(),
            reason: entry.state.reason_code().map(str::to_owned),
            describe: entry.state.describe(),
            catalogue: CatalogueDto::from(&entry.catalogue),
            root_agent: entry.root_agent,
            runner_installed: entry.runner_installed,
            from_hello_skills: entry.from_hello_skills,
        }
    }
}

/// One host's whole inventory, plus the ids the agent reported that the hub does not know.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CapabilitiesDto {
    /// The host's name.
    pub host: String,
    /// One row per capability this hub knows.
    pub entries: Vec<CapabilityDto>,
    /// Ids the agent reported that no contract describes. Diagnostics only.
    pub unknown_contracts: Vec<String>,
}

/// The capability inventory of host `name`.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/capabilities", operation_id = "list_capabilities",
    tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "The inventory", body = CapabilitiesDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn list_capabilities(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<CapabilitiesDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let entries = state.deps.capabilities.inventory(host.id).await;
    Ok(Json(CapabilitiesDto {
        host: host.name.to_string(),
        entries: entries.into_iter().map(CapabilityDto::from).collect(),
        unknown_contracts: state.deps.capabilities.unknown_contracts(host.id),
    }))
}
