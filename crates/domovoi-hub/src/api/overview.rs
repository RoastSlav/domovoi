//! The overview: the handful of counts the navigation shows beside its links, for every
//! host or for one, so the UI does not fetch whole lists to count them.

use axum::Json;
use axum::extract::{Query, State};
use domovoi_core::ports::{IncidentFilter, OperationFilter, Page, Presence};
use domovoi_core::{Host, Incident, IncidentGroup, IncidentStatus, Verdict};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::error::{ApiError, ErrorBody};
use super::snapshot::snapshot_seq;
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
    /// The incidents that are not resolved, counted by the group the list filters on.
    pub incident_groups: IncidentGroupsDto,
    /// Hosts with an enrolled agent that is not connected.
    pub hosts_offline: u32,
    /// Hosts counted.
    pub hosts_total: u32,
    /// Operations that are running.
    pub operations_running: u32,
    /// Operations with a step waiting for a person to approve it (operations, not steps).
    pub operations_waiting_approval: u32,
    /// Operations waiting for the person to answer a question.
    pub operations_waiting_user: u32,
    /// Operations that are paused, for whatever reason, including an unknown result.
    pub operations_paused: u32,
    /// Everything that waits for the owner: incidents that need action, approvals, questions,
    /// paused operations, unknown results and offline hosts.
    pub attention_total: u32,
    /// The journal position this snapshot was read at; apply only events with a greater `seq`.
    pub as_of_seq: u64,
}

/// How many unresolved incidents are in each group of the incident list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct IncidentGroupsDto {
    /// Open, and triage did not say to only watch it or that it is expected.
    pub needs_action: u32,
    /// Open, and triage said to keep an eye on it.
    pub monitor: u32,
    /// Not confirmed yet.
    pub pending: u32,
    /// Open, and expected: triage said so, or a known condition does.
    pub expected: u32,
    /// Acknowledged or muted.
    pub handled: u32,
}

impl IncidentGroupsDto {
    fn of(incidents: &[Incident]) -> Self {
        let in_group = |group| count(incidents.iter().filter(|i| i.group() == group).count());
        Self {
            needs_action: in_group(IncidentGroup::NeedsAction),
            monitor: in_group(IncidentGroup::Monitor),
            pending: in_group(IncidentGroup::Pending),
            expected: in_group(IncidentGroup::Expected),
            handled: in_group(IncidentGroup::Handled),
        }
    }
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

/// How many operations are in `status`, for `only` host when given.
async fn operations_in(
    state: &ApiState,
    status: &str,
    only: Option<domovoi_core::HostId>,
    page: &Page,
) -> Result<usize, ApiError> {
    let filter = OperationFilter {
        status: Some(status.to_owned()),
        host: only,
        ..OperationFilter::default()
    };
    Ok(state
        .deps
        .operation_repo
        .list(&filter, page)
        .await?
        .items
        .len())
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
    let as_of_seq = snapshot_seq(&state).await?;
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

    let running = operations_in(&state, "running", only, &page).await?;
    let waiting_approval = operations_in(&state, "waiting_approval", only, &page).await?;
    let waiting_user = operations_in(&state, "waiting_user", only, &page).await?;
    let paused = operations_in(&state, "paused", only, &page).await?;

    let mut offline = 0_usize;
    for host in &hosts {
        if is_offline(&state, host).await? {
            offline += 1;
        }
    }

    let groups = IncidentGroupsDto::of(&incidents);
    let attention_total = [
        groups.needs_action,
        count(waiting_approval),
        count(waiting_user),
        count(paused),
        count(offline),
    ]
    .into_iter()
    .fold(0_u32, u32::saturating_add);

    Ok(Json(OverviewDto {
        incidents_action_needed: count(action_needed),
        incidents_open: count(incidents.len()),
        incident_groups: groups,
        hosts_offline: count(offline),
        hosts_total: count(hosts.len()),
        operations_running: count(running),
        operations_waiting_approval: count(waiting_approval),
        operations_waiting_user: count(waiting_user),
        operations_paused: count(paused),
        attention_total,
        as_of_seq,
    }))
}
