//! The query parameters of the list endpoints.

use serde::Deserialize;
use utoipa::IntoParams;

/// Filters for the incident list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct IncidentQuery {
    /// Only this status: `pending`, `open`, `acked`, `muted` or `resolved`.
    pub status: Option<String>,
    /// Only this host, by name.
    pub host: Option<String>,
    /// Only this severity: `info`, `warning` or `critical`.
    pub severity: Option<String>,
    /// Only this group: `needs_action`, `monitor`, `pending`, `expected`, `handled` or
    /// `resolved`. An open incident is in `monitor` or `expected` when triage said so, and
    /// in `needs_action` otherwise; `handled` is acknowledged or muted.
    pub group: Option<String>,
    /// Only incidents whose title, check or target contains this text, ignoring case.
    pub q: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 200. Default 50.
    pub limit: Option<u16>,
}

/// Options for running triage again.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RetryQuery {
    /// Run the model again even when a cached report would fit. Default false.
    pub fresh: Option<bool>,
}

/// Filters for the log lines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LogsQuery {
    /// How many lines to return, 1 to 2000. Default 200.
    pub lines: Option<u16>,
    /// The least severe level to return: `error`, `warn`, `info`, `debug` or `trace`.
    pub level: Option<String>,
}

/// Filters for the action list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActionQuery {
    /// Only this status name, for example `proposed`.
    pub status: Option<String>,
    /// Only actions on this host, by name.
    pub host: Option<String>,
    /// `true`: only actions that are no longer waiting for approval; `false`: only those
    /// that are.
    pub decided: Option<bool>,
    /// Only actions that last changed at or after this time, RFC 3339.
    pub since: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 200. Default 50.
    pub limit: Option<u16>,
}
