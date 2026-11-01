//! Prometheus Alertmanager webhooks (version 4): one signal per alert.

use std::collections::BTreeMap;

use domovoi_core::{CheckKey, Evidence, EvidenceValue, Severity, SignalStatus};
use serde::Deserialize;
use serde_json::Value;

use super::{
    ParseError, ParseOptions, SourceSignal, clip, host_name, key_segment, put, target, time,
};

/// Labels that change between otherwise identical alerts, so they would split one
/// problem into many incidents. They are left out of the evidence.
pub const VOLATILE_LABELS: &[&str] = &[
    "fingerprint",
    "pod",
    "pod_name",
    "pod_template_hash",
    "controller_revision_hash",
    "container_id",
];

#[derive(Deserialize)]
struct Payload {
    alerts: Vec<Alert>,
}

#[derive(Deserialize)]
struct Alert {
    status: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(rename = "startsAt")]
    starts_at: Option<String>,
    #[serde(rename = "endsAt")]
    ends_at: Option<String>,
}

pub(super) fn parse(
    value: Value,
    options: ParseOptions<'_>,
) -> Result<Vec<SourceSignal>, ParseError> {
    let payload: Payload =
        serde_json::from_value(value).map_err(|e| ParseError::Malformed(e.to_string()))?;
    if payload.alerts.len() > super::MAX_SIGNALS {
        return Err(ParseError::TooMany(payload.alerts.len()));
    }
    payload
        .alerts
        .into_iter()
        .enumerate()
        .map(|(i, alert)| one(alert, i, options))
        .collect()
}

fn one(alert: Alert, i: usize, options: ParseOptions<'_>) -> Result<SourceSignal, ParseError> {
    let labels = &alert.labels;
    let name = labels.get("alertname").map_or("unknown", String::as_str);
    let check = CheckKey::parse(&format!("alertmanager.{}", key_segment(name))).map_err(|e| {
        ParseError::Invalid {
            field: format!("alerts[{i}].labels.alertname"),
            reason: e.to_string(),
        }
    })?;
    let status = match alert.status.as_str() {
        "firing" => SignalStatus::Firing,
        "resolved" => SignalStatus::Resolved,
        other => {
            return Err(ParseError::Invalid {
                field: format!("alerts[{i}].status"),
                reason: format!("{other:?} is not \"firing\" or \"resolved\""),
            });
        }
    };
    let severity = match labels.get("severity").map(|s| s.to_ascii_lowercase()) {
        Some(s) if s == "critical" => Severity::Critical,
        Some(s) if s == "warning" => Severity::Warning,
        _ => Severity::Info,
    };
    let target = labels
        .get("instance")
        .or_else(|| labels.get("job"))
        .and_then(|t| target(t));
    let host = match options.host_label {
        Some(label) => labels.get(label).and_then(|h| host_name(h)),
        None => labels
            .get("host")
            .and_then(|h| host_name(h))
            .or_else(|| labels.get("instance").and_then(|h| host_name(h))),
    };
    let summary = alert
        .annotations
        .get("summary")
        .or_else(|| alert.annotations.get("description"))
        .map_or(name, String::as_str);
    let mut evidence = Evidence::new();
    for (key, value) in labels {
        if !VOLATILE_LABELS.contains(&key.as_str()) {
            put(&mut evidence, key, EvidenceValue::Text(value.clone()));
        }
    }
    let observed_at = match status {
        SignalStatus::Firing => time(alert.starts_at.as_deref()),
        SignalStatus::Resolved => time(alert.ends_at.as_deref()),
    };
    Ok(SourceSignal {
        host,
        check,
        target,
        status,
        severity,
        summary: clip(summary.trim(), domovoi_core::signal::SUMMARY_MAX_CHARS),
        evidence,
        observed_at,
    })
}
