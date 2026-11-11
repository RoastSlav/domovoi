//! The overview: the handful of counts the navigation shows beside its links, for every
//! host or for one, so the UI does not fetch whole lists to count them.

use axum::Json;
use axum::extract::{Query, State};
use domovoi_core::ports::{ActionFilter, IncidentFilter, Page, Presence};
use domovoi_core::{Host, IncidentStatus, Verdict};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, MAX_PAGE, host_named};

/// Which hosts the overview counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OverviewQuery {
    /// Only this host, by name. Every host when absent.
    pub host: Option<String>,
}

/// What needs the operator right now, as counts. Each count stops at 200.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct OverviewDto {
    /// Open incidents nobody has acknowledged and triage did not judge expected.
    pub incidents_action_needed: u32,
    /// Incidents that are not resolved: pending, open, acknowledged or muted.
    pub incidents_open: u32,
    /// Actions waiting for a person to approve or deny them.
    pub approvals_waiting: u32,
    /// Hosts with an enrolled agent that is not connected.
    pub hosts_offline: u32,
    /// Hosts counted.
    pub hosts_total: u32,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Whether `host` has an enrolled agent that is not connected now. The hub's own machine
/// without an agent, and a host that was only invited, are not offline.
async fn is_offline(state: &ApiState, host: &Host) -> Result<bool, ApiError> {
    if matches!(
        state.deps.agents.presence(host.id),
        Presence::Connected { .. }
    ) {
        return Ok(false);
    }
    Ok(state.deps.hosts.live_agent(host.id).await?.is_some())
}

/// Counts for the navigation: incidents, approvals and offline hosts.
#[utoipa::path(
    get, path = "/api/v1/overview", operation_id = "overview", tag = "system",
    params(OverviewQuery),
    responses(
        (status = 200, description = "The counts", body = OverviewDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn show(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<OverviewDto>, ApiError> {
    let hosts = match &query.host {
        Some(name) => vec![host_named(&state, name).await?],
        None => state.deps.hosts.list_hosts().await?,
    };
    let only = query
        .host
        .as_ref()
        .and_then(|_| hosts.first().map(|h| h.id));
    let page = Page {
        cursor: None,
        limit: MAX_PAGE,
    };

    let active = IncidentFilter {
        host: only,
        active: Some(true),
        ..IncidentFilter::default()
    };
    let incidents = state.deps.incidents.list(&active, &page).await?.items;
    let action_needed = incidents
        .iter()
        .filter(|i| {
            matches!(i.status(), IncidentStatus::Open) && i.verdict() != Some(Verdict::Expected)
        })
        .count();

    let proposed = ActionFilter {
        host: only,
        status: Some("proposed".to_owned()),
        ..ActionFilter::default()
    };
    let waiting = state.deps.actions.list(&proposed, &page).await?.items.len();

    let mut offline = 0_usize;
    for host in &hosts {
        if is_offline(&state, host).await? {
            offline += 1;
        }
    }

    Ok(Json(OverviewDto {
        incidents_action_needed: count(action_needed),
        incidents_open: count(incidents.len()),
        approvals_waiting: count(waiting),
        hosts_offline: count(offline),
        hosts_total: count(hosts.len()),
    }))
}
