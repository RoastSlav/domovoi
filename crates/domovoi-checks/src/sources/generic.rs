//! Domovoi's generic webhook format.
//!
//! ```json
//! { "host": "nas", "check": "backup.nightly", "target": "photos", "status": "firing",
//!   "severity": "warning", "summary": "Backup failed", "evidence": { "exit_code": 2 } }
//! ```
//!
//! An array of these is accepted too. Only `check` and `summary` are required; status
//! defaults to firing and severity to warning.

use domovoi_core::{CheckKey, Evidence, EvidenceValue, Severity, SignalStatus};
use serde::Deserialize;
use serde_json::Value;

use super::{ParseError, SourceSignal, clip, put, severity, target, time};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generic {
    host: Option<String>,
    check: String,
    target: Option<String>,
    status: Option<String>,
    severity: Option<String>,
    summary: String,
    #[serde(default)]
    evidence: serde_json::Map<String, Value>,
    observed_at: Option<String>,
}

pub(super) fn parse(value: Value) -> Result<Vec<SourceSignal>, ParseError> {
    match value {
        Value::Array(items) => {
            if items.len() > super::MAX_SIGNALS {
                return Err(ParseError::TooMany(items.len()));
            }
            items
                .into_iter()
                .enumerate()
                .map(|(i, item)| one(item, &format!("[{i}].")))
                .collect()
        }
        object @ Value::Object(_) => Ok(vec![one(object, "")?]),
        _ => Err(ParseError::Malformed(
            "expected an object or an array of objects".to_owned(),
        )),
    }
}

fn invalid(prefix: &str, field: &str, reason: impl Into<String>) -> ParseError {
    ParseError::Invalid {
        field: format!("{prefix}{field}"),
        reason: reason.into(),
    }
}

fn one(value: Value, prefix: &str) -> Result<SourceSignal, ParseError> {
    let raw: Generic = serde_json::from_value(value)
        .map_err(|e| ParseError::Malformed(format!("{prefix}{e}")))?;
    let check = CheckKey::parse(&raw.check).map_err(|e| invalid(prefix, "check", e.to_string()))?;
    let target = match raw.target.as_deref() {
        None => None,
        Some(text) => Some(target(text).ok_or_else(|| {
            invalid(prefix, "target", "must be 1 to 128 printable characters")
        })?),
    };
    let status = match raw.status.as_deref().map(str::trim) {
        None | Some("firing") => SignalStatus::Firing,
        Some("resolved") => SignalStatus::Resolved,
        Some(other) => {
            return Err(invalid(
                prefix,
                "status",
                format!("{other:?} is not \"firing\" or \"resolved\""),
            ));
        }
    };
    let severity = match raw.severity.as_deref() {
        None => Severity::Warning,
        Some(text) => severity(text).ok_or_else(|| {
            invalid(
                prefix,
                "severity",
                format!("{text:?} is not \"info\", \"warning\" or \"critical\""),
            )
        })?,
    };
    if raw.summary.trim().is_empty() {
        return Err(invalid(prefix, "summary", "must not be empty"));
    }
    let mut evidence = Evidence::new();
    for (key, value) in raw.evidence {
        let value = match value {
            Value::String(text) => EvidenceValue::Text(text),
            Value::Bool(b) => EvidenceValue::Bool(b),
            Value::Number(n) => match (n.as_i64(), n.as_f64()) {
                (Some(i), _) => EvidenceValue::Int(i),
                (None, Some(f)) => EvidenceValue::Float(f),
                (None, None) => EvidenceValue::Text(n.to_string()),
            },
            _ => {
                return Err(invalid(
                    prefix,
                    &format!("evidence.{key}"),
                    "must be a string, number or boolean",
                ));
            }
        };
        put(&mut evidence, &key, value);
    }
    Ok(SourceSignal {
        host: raw
            .host
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty()),
        check,
        target,
        status,
        severity,
        summary: clip(raw.summary.trim(), domovoi_core::signal::SUMMARY_MAX_CHARS),
        evidence,
        observed_at: time(raw.observed_at.as_deref()),
    })
}
