//! Parsers for alerts that other tools send as webhooks (feature `sources`).
//!
//! Pure: bytes in, [`SourceSignal`]s out. The hub's `/hooks/{name}` endpoints and the
//! agent's local relay share them. A payload names hosts by *name*; the receiver turns
//! names into host ids.

mod alertmanager;
mod generic;
mod uptime_kuma;

use domovoi_core::{
    CheckKey, Evidence, EvidenceValue, HostId, Severity, Signal, SignalStatus, SourceKind,
    TargetKey,
};
use jiff::Timestamp;

/// The largest payload accepted.
pub const MAX_BODY_BYTES: usize = 1 << 20;

/// The most signals one payload may carry.
pub const MAX_SIGNALS: usize = 100;

/// The most evidence entries kept per signal; later ones in key order are dropped.
pub const MAX_EVIDENCE_KEYS: usize = 32;

/// The longest evidence key or text value kept, in characters.
pub const MAX_EVIDENCE_CHARS: usize = 1024;

/// A payload format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// Domovoi's own JSON schema: one object or an array of them.
    Generic,
    /// A Prometheus Alertmanager webhook (version 4).
    Alertmanager,
    /// An Uptime Kuma webhook notification.
    UptimeKuma,
}

impl SourceFormat {
    /// Every format, in documentation order.
    pub const ALL: [Self; 3] = [Self::Generic, Self::Alertmanager, Self::UptimeKuma];

    /// The format's name in config, paths and `SourceKind::Webhook { kind }`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Alertmanager => "alertmanager",
            Self::UptimeKuma => "uptime_kuma",
        }
    }

    /// The format called `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == name)
    }
}

/// Settings that change how a payload is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOptions<'a> {
    /// For Alertmanager, the label that names the host.
    pub host_label: Option<&'a str>,
}

/// Why a payload was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The body is larger than [`MAX_BODY_BYTES`].
    #[error("the payload is {0} bytes, over the {MAX_BODY_BYTES} byte limit")]
    TooLarge(usize),
    /// The payload carries more than [`MAX_SIGNALS`] signals.
    #[error("the payload has {0} alerts, over the limit of {MAX_SIGNALS}")]
    TooMany(usize),
    /// The body is not JSON of the expected shape.
    #[error("the payload is not valid: {0}")]
    Malformed(String),
    /// A field holds a value Domovoi cannot use.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field, such as `check` or `[2].severity`.
        field: String,
        /// What is wrong with it.
        reason: String,
    },
}

/// One alert read from a payload, about a host known only by name.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceSignal {
    /// The host the payload names, lowercased; `None` when it names none.
    pub host: Option<String>,
    /// What was checked.
    pub check: CheckKey,
    /// What it was checked on.
    pub target: Option<TargetKey>,
    /// Firing or resolved.
    pub status: SignalStatus,
    /// How bad, when firing.
    pub severity: Severity,
    /// One line for people.
    pub summary: String,
    /// Structured details.
    pub evidence: Evidence,
    /// When the sender saw it, if it said.
    pub observed_at: Option<Timestamp>,
}

impl SourceSignal {
    /// The signal for host `host`, received at `now`. A sender's time later than `now` is
    /// replaced by `now`.
    #[must_use]
    pub fn into_signal(self, host: HostId, format: SourceFormat, now: Timestamp) -> Signal {
        let source = SourceKind::Webhook {
            kind: format.name().to_owned(),
        };
        let at = self.observed_at.filter(|t| *t <= now).unwrap_or(now);
        let signal = match self.status {
            SignalStatus::Firing => Signal::firing(
                source,
                host,
                self.check,
                self.target,
                self.severity,
                self.summary,
                at,
            ),
            SignalStatus::Resolved => {
                Signal::resolved(source, host, self.check, self.target, self.summary, at)
            }
        };
        signal.with_evidence(self.evidence)
    }
}

/// Reads `body` as `format`.
pub fn parse(
    format: SourceFormat,
    body: &[u8],
    options: ParseOptions<'_>,
) -> Result<Vec<SourceSignal>, ParseError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(ParseError::TooLarge(body.len()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| ParseError::Malformed(e.to_string()))?;
    let signals = match format {
        SourceFormat::Generic => generic::parse(value)?,
        SourceFormat::Alertmanager => alertmanager::parse(value, options)?,
        SourceFormat::UptimeKuma => uptime_kuma::parse(value)?,
    };
    if signals.len() > MAX_SIGNALS {
        return Err(ParseError::TooMany(signals.len()));
    }
    Ok(signals)
}

/// `text` cut to `max` characters, with an ellipsis when cut.
pub(crate) fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `raw` as one check-key segment: lowercase, anything else replaced by `_`.
pub(crate) fn key_segment(raw: &str) -> String {
    let segment: String = raw
        .trim()
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    if segment.is_empty() {
        "unknown".to_owned()
    } else {
        segment
    }
}

/// A target from free text: trimmed, control characters removed, at most 128 characters.
pub(crate) fn target(raw: &str) -> Option<TargetKey> {
    let cleaned: String = raw.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    TargetKey::parse(&clip(cleaned, 128)).ok()
}

/// A severity name.
pub(crate) fn severity(raw: &str) -> Option<Severity> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "info" => Some(Severity::Info),
        "warning" => Some(Severity::Warning),
        "critical" => Some(Severity::Critical),
        _ => None,
    }
}

/// Adds `key = value` to `evidence`, clipped, unless the entry limit is reached.
pub(crate) fn put(evidence: &mut Evidence, key: &str, value: EvidenceValue) {
    if evidence.iter().count() >= MAX_EVIDENCE_KEYS {
        return;
    }
    let value = match value {
        EvidenceValue::Text(text) => EvidenceValue::Text(clip(&text, MAX_EVIDENCE_CHARS)),
        other => other,
    };
    evidence.insert(clip(key, 64), value);
}

/// A host name from free text: lowercased, port and domain removed. `None` for IP
/// addresses, which say nothing about which host Domovoi knows.
pub(crate) fn host_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let without_port = match raw.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => raw,
    };
    let without_port = without_port.trim_matches(['[', ']']);
    if without_port.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let first = without_port.split('.').next().unwrap_or(without_port);
    let name = first.to_ascii_lowercase();
    (!name.is_empty()).then_some(name)
}

/// When the sender saw it, from an RFC 3339 time. Missing, unreadable and placeholder
/// times (before 2000) give `None`.
pub(crate) fn time(raw: Option<&str>) -> Option<Timestamp> {
    let at: Timestamp = raw?.parse().ok()?;
    (at.as_second() >= 946_684_800).then_some(at)
}
