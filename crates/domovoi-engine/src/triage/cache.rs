//! The repeat cache key: what makes two incidents' evidence "equivalent".

use domovoi_core::{EvidenceValue, Incident, Signal};
use sha2::{Digest, Sha256};

/// A number bucketed to two significant figures, so 92.4 and 92.1 match but 92 and 95 do
/// not.
fn bucket(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return format!("{value}");
    }
    format!("{value:.1e}")
}

/// Whether `text` is a timestamp, which changes every time and says nothing about cause.
fn is_time(text: &str) -> bool {
    text.parse::<jiff::Timestamp>().is_ok() || text.parse::<jiff::civil::DateTime>().is_ok()
}

/// The digest of `incident`'s evidence as of its `latest` signal: check, target and every
/// evidence entry in key order, with numbers bucketed to two significant figures and
/// timestamps dropped.
#[must_use]
pub(crate) fn evidence_digest(incident: &Incident, latest: Option<&Signal>) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"triage-evidence:v1\n");
    hash.update(incident.check().as_str().as_bytes());
    hash.update(b"\n");
    hash.update(incident.target().map_or("", |t| t.as_str()).as_bytes());
    hash.update(b"\n");
    if let Some(signal) = latest {
        for (key, value) in signal.evidence().iter() {
            let value = match value {
                // Precision loss above 2^53 does not matter for a two-figure bucket.
                EvidenceValue::Int(n) => bucket(*n as f64),
                EvidenceValue::Float(f) => bucket(*f),
                EvidenceValue::Bool(b) => b.to_string(),
                EvidenceValue::Text(t) if is_time(t) => continue,
                EvidenceValue::Text(t) => t.clone(),
            };
            hash.update(key.as_bytes());
            hash.update(b"=");
            hash.update(value.as_bytes());
            hash.update(b"\n");
        }
    }
    hash.finalize().into()
}
