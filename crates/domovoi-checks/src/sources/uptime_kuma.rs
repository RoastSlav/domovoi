//! Uptime Kuma webhook notifications.
//!
//! A heartbeat with status 0 is a monitor going down and 1 is it coming back. Pending
//! (2) and maintenance (3) heartbeats, and test notifications without a heartbeat, give
//! no signal.

use domovoi_core::{CheckKey, Evidence, EvidenceValue, Severity, SignalStatus};
use serde::Deserialize;
use serde_json::Value;

use super::{ParseError, SourceSignal, clip, put, target};

/// The check every Uptime Kuma monitor reports as; the monitor name is the target.
pub const CHECK: &str = "uptime_kuma.monitor";

#[derive(Deserialize)]
struct Payload {
    heartbeat: Option<Heartbeat>,
    monitor: Option<Monitor>,
    msg: Option<String>,
}

#[derive(Deserialize)]
struct Heartbeat {
    status: i64,
    msg: Option<String>,
}

#[derive(Deserialize)]
struct Monitor {
    name: Option<String>,
    url: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

pub(super) fn parse(value: Value) -> Result<Vec<SourceSignal>, ParseError> {
    let payload: Payload =
        serde_json::from_value(value).map_err(|e| ParseError::Malformed(e.to_string()))?;
    let Some(heartbeat) = payload.heartbeat else {
        return Ok(Vec::new());
    };
    let status = match heartbeat.status {
        0 => SignalStatus::Firing,
        1 => SignalStatus::Resolved,
        _ => return Ok(Vec::new()),
    };
    let check = CheckKey::parse(CHECK).map_err(|e| ParseError::Invalid {
        field: "check".to_owned(),
        reason: e.to_string(),
    })?;
    let monitor = payload.monitor;
    let name = monitor.as_ref().and_then(|m| m.name.as_deref());
    let mut evidence = Evidence::new();
    if let Some(monitor) = &monitor {
        if let Some(url) = monitor
            .url
            .as_deref()
            .filter(|u| !u.is_empty() && *u != "https://")
        {
            put(&mut evidence, "url", EvidenceValue::Text(url.to_owned()));
        }
        if let Some(kind) = &monitor.kind {
            put(&mut evidence, "type", EvidenceValue::Text(kind.clone()));
        }
    }
    let message = heartbeat
        .msg
        .filter(|m| !m.trim().is_empty())
        .or(payload.msg)
        .unwrap_or_default();
    let summary = if message.trim().is_empty() {
        match status {
            SignalStatus::Firing => "A monitor is down".to_owned(),
            SignalStatus::Resolved => "A monitor is up".to_owned(),
        }
    } else {
        message.trim().to_owned()
    };
    Ok(vec![SourceSignal {
        host: None,
        check,
        target: name.and_then(target),
        status,
        severity: Severity::Warning,
        summary: clip(&summary, domovoi_core::signal::SUMMARY_MAX_CHARS),
        evidence,
        observed_at: None,
    }])
}
