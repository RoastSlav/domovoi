//! Building the daily digest. Pure.
//!
//! The same lines every morning are noise, so a host with nothing new collapses to one
//! line. Other hosts list what changed since the previous digest, with a projection when
//! free space is running out.

use std::collections::BTreeMap;

use domovoi_core::{
    Advisory, Digest, DigestFact, FactValue, Host, HostDigest, HostId, Incident, Trend,
};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use super::trend::{projection, projection_text};
use crate::notify::human;

/// A change smaller than this fraction of the previous value is not a change.
pub const TOLERANCE: f64 = 0.01;
/// The span deltas are reported over.
pub const DELTA_WINDOW: SignedDuration = SignedDuration::from_hours(24 * 7);

/// Whether a fact counts down towards zero, so a decline can be projected.
#[must_use]
pub fn has_floor(fact: &DigestFact) -> bool {
    let key = fact.key.as_str();
    key.ends_with(".free_bytes")
        || key.ends_with(".free")
        || key.ends_with(".available")
        || (fact.unit.as_deref() == Some("bytes") && fact.label.to_lowercase().contains("free"))
}

/// A value with its unit, readable: bytes in GB, percentages without a space.
#[must_use]
pub fn format_value(value: f64, unit: Option<&str>) -> String {
    let num = |v: f64| {
        if v.abs() >= 100.0 || v.fract() == 0.0 {
            format!("{v:.0}")
        } else {
            format!("{v:.1}")
        }
    };
    match unit {
        Some("bytes") => format!("{} GB", num(value / 1e9)),
        Some("%") => format!("{}%", num(value)),
        Some(unit) if !unit.is_empty() => format!("{} {unit}", num(value)),
        _ => num(value),
    }
}

fn number(fact: &DigestFact) -> Option<f64> {
    match fact.value {
        FactValue::Number(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

fn day_start(date: Date, tz: &TimeZone) -> Timestamp {
    date.to_zoned(tz.clone())
        .map(|z| z.timestamp())
        .unwrap_or(Timestamp::MIN)
}

/// The latest fact per key.
fn latest(facts: &[&DigestFact]) -> BTreeMap<String, DigestFact> {
    let mut out: BTreeMap<String, DigestFact> = BTreeMap::new();
    for fact in facts {
        match out.get(&fact.key) {
            Some(seen) if seen.observed_at > fact.observed_at => {}
            _ => {
                out.insert(fact.key.clone(), (*fact).clone());
            }
        }
    }
    out
}

fn moved(now: f64, before: f64) -> bool {
    (now - before).abs() > TOLERANCE * before.abs().max(f64::EPSILON)
}

/// The digest line and trend for one fact that changed, if it changed.
fn change(
    today: &DigestFact,
    before: Option<&DigestFact>,
    history: &[(Timestamp, f64)],
    now: Timestamp,
) -> Option<(String, Option<Trend>)> {
    let before = before?;
    let unit = today.unit.as_deref();
    match (number(today), number(before)) {
        (Some(value), Some(prev)) => {
            // A slow decline still matters when it runs out within the projection limit.
            let left = if has_floor(today) {
                projection(history, 0.0, now)
            } else {
                None
            };
            if !moved(value, prev) && left.is_none() {
                return None;
            }
            let since = now.checked_sub(DELTA_WINDOW).unwrap_or(Timestamp::MIN);
            let (from_at, from) = history
                .iter()
                .filter(|(at, _)| *at >= since)
                .min_by_key(|(at, _)| *at)
                .copied()
                .unwrap_or((before.observed_at, prev));
            let delta = value - from;
            let period = today.observed_at.duration_since(from_at);
            let direction = if delta < 0.0 { "down" } else { "up" };
            let mut line = format!(
                "{} {}, {direction} {} in {}",
                today.label,
                format_value(value, unit),
                format_value(delta.abs(), unit),
                human(period)
            );
            if let Some(left) = left {
                line.push_str(", ");
                line.push_str(&projection_text(left));
            }
            let trend = Trend {
                key: today.key.clone(),
                delta,
                period,
                projection: left.map(projection_text),
            };
            Some((line, Some(trend)))
        }
        _ if today.value != before.value => {
            let text = |v: &FactValue| match v {
                FactValue::Text(t) => t.clone(),
                FactValue::Number(n) => format_value(*n, unit),
            };
            Some((
                format!(
                    "{}: {} (was {})",
                    today.label,
                    text(&today.value),
                    text(&before.value)
                ),
                None,
            ))
        }
        _ => None,
    }
}

/// One host's part of the digest.
fn host_digest(
    host: &Host,
    today: &[&DigestFact],
    history: &[&DigestFact],
    open: usize,
    new_advisories: usize,
    start: Timestamp,
    now: Timestamp,
) -> HostDigest {
    let current = latest(today);
    let earlier: Vec<&DigestFact> = history
        .iter()
        .copied()
        .filter(|f| f.observed_at < start)
        .collect();
    let previous = latest(&earlier);
    let mut lines = Vec::new();
    let mut trends = Vec::new();
    for (key, fact) in &current {
        let points: Vec<(Timestamp, f64)> = history
            .iter()
            .chain(today.iter())
            .filter(|f| f.key == *key)
            .filter_map(|f| number(f).map(|v| (f.observed_at, v)))
            .collect();
        if let Some((line, trend)) = change(fact, previous.get(key), &points, now) {
            lines.push(line);
            trends.extend(trend);
        }
    }
    let quiet = lines.is_empty() && open == 0 && new_advisories == 0;
    HostDigest {
        host: host.name.clone(),
        quiet,
        lines,
        trends,
    }
}

/// What a digest is built from.
#[derive(Debug, Clone, Copy)]
pub struct DigestInputs<'a> {
    /// The day the digest is for, in `tz`.
    pub date: Date,
    /// Every host.
    pub hosts: &'a [Host],
    /// Facts observed on `date`.
    pub facts_today: &'a [DigestFact],
    /// Facts from the 14 days before.
    pub facts_history: &'a [DigestFact],
    /// Active incidents.
    pub open_incidents: &'a [Incident],
    /// Advisories; resolved ones are left out.
    pub advisories: &'a [Advisory],
    /// The configured time zone, for day boundaries.
    pub tz: &'a TimeZone,
    /// When the digest is built, for ages and projections.
    pub now: Timestamp,
}

/// The digest described by `inputs`.
#[must_use]
pub fn build_digest(inputs: &DigestInputs<'_>) -> Digest {
    let DigestInputs {
        date,
        hosts,
        facts_today,
        facts_history,
        open_incidents,
        advisories,
        tz,
        now,
    } = *inputs;
    let start = day_start(date, tz);
    let names: BTreeMap<HostId, &Host> = hosts.iter().map(|h| (h.id, h)).collect();
    let mut sorted: Vec<&Host> = hosts.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let host_digests = sorted
        .into_iter()
        .map(|host| {
            let today: Vec<&DigestFact> =
                facts_today.iter().filter(|f| f.host == host.id).collect();
            let history: Vec<&DigestFact> =
                facts_history.iter().filter(|f| f.host == host.id).collect();
            let open = open_incidents
                .iter()
                .filter(|i| i.host() == host.id)
                .count();
            let new_advisories = advisories
                .iter()
                .filter(|a| a.host == Some(host.id) && a.created_at >= start)
                .count();
            host_digest(host, &today, &history, open, new_advisories, start, now)
        })
        .collect();
    let mut incidents: Vec<&Incident> = open_incidents.iter().collect();
    incidents.sort_by_key(|i| i.opened_at());
    let open_lines = incidents
        .into_iter()
        .map(|i| {
            let host = names
                .get(&i.host())
                .map_or("unknown host", |h| h.name.as_str());
            let state = i
                .verdict()
                .map_or_else(|| i.severity().to_string(), |v| v.label().to_lowercase());
            let age = human(now.duration_since(i.opened_at()));
            (
                i.id(),
                format!("{host}: {} (open {age}, {state})", i.title()),
            )
        })
        .collect();
    let mut active: Vec<Advisory> = advisories
        .iter()
        .filter(|a| a.resolved_at.is_none())
        .cloned()
        .collect();
    active.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    Digest {
        date,
        hosts: host_digests,
        open_incidents: open_lines,
        advisories: active,
    }
}
