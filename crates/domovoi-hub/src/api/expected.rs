//! Signals a known condition explained. They opened no incident, so this is where a person
//! sees that they happened: on the host page and in Search.

use axum::Json;
use axum::extract::{Query, State};
use domovoi_core::MATCHED_RULE_KEY;
use domovoi_core::{EvidenceValue, Signal};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::error::{ApiError, ErrorBody};
use super::rules::host_limit;
use super::{ApiState, Authed};

/// Most rows one listing returns.
const MAX_ROWS: u16 = 500;

/// Filters for the expected-signal listing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ExpectedQuery {
    /// Only this host, by name.
    pub host: Option<String>,
    /// How many rows to return, 1 to 500. Default 100.
    pub limit: Option<u16>,
}

/// One event a known condition explained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExpectedSignalDto {
    /// The signal's id.
    pub id: String,
    /// The host's name.
    pub host: String,
    /// The check that reported it.
    pub check: String,
    /// What it was about, such as `tcp:0.0.0.0:22`.
    pub target: Option<String>,
    /// What the check said.
    pub summary: String,
    /// When the check observed it, RFC 3339.
    pub observed_at: String,
    /// The known condition that explained it.
    pub rule_id: String,
    /// The reason that condition gives, when it still exists.
    pub rule_note: Option<String>,
}

fn rule_of(signal: &Signal) -> Option<String> {
    match signal.evidence().get(MATCHED_RULE_KEY) {
        Some(EvidenceValue::Text(id)) => Some(id.clone()),
        _ => None,
    }
}

/// Events that known conditions explained, newest first. They are history: no incident,
/// notification or triage was made for them.
#[utoipa::path(
    get, path = "/api/v1/expected-signals", operation_id = "list_expected_signals", tag = "incidents",
    params(ExpectedQuery),
    responses(
        (status = 200, description = "Signals a known condition explained, newest first", body = Vec<ExpectedSignalDto>),
        (status = 400, description = "Unknown host", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
    Query(query): Query<ExpectedQuery>,
) -> Result<Json<Vec<ExpectedSignalDto>>, ApiError> {
    let host = host_limit(&state, query.host.as_ref()).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_ROWS);
    let signals = state.deps.signals.list_matched(host, limit).await?;
    let rules = state.deps.acks.list_rules().await?;
    let mut rows = Vec::with_capacity(signals.len());
    for signal in &signals {
        let Some(rule_id) = rule_of(signal) else {
            continue;
        };
        let name = match state.deps.hosts.get_host(signal.host()).await? {
            Some(host) => host.name.to_string(),
            None => format!("unknown host {}", signal.host()),
        };
        rows.push(ExpectedSignalDto {
            id: signal.id().to_string(),
            host: name,
            check: signal.check().as_str().to_owned(),
            target: signal.target().map(|t| t.as_str().to_owned()),
            summary: state.deps.redactor.redact(signal.summary()),
            observed_at: signal.observed_at().to_string(),
            rule_note: rules
                .iter()
                .find(|rule| rule.id.to_string() == rule_id)
                .map(|rule| rule.note.clone()),
            rule_id,
        });
    }
    Ok(Json(rows))
}
