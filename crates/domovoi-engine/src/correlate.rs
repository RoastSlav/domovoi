//! Which incident a signal belongs to. Pure: no I/O, no clock.
//!
//! Rules, in order:
//! 1. An active incident with the same fingerprint (or one grouped into it): `Existing`.
//! 2. A firing signal whose incident resolved within the reopen window: `Reopen`.
//! 3. A firing signal about the same target as an active incident on the same host, from
//!    another source, opened within the grouping window: `Existing`, grouped.
//! 4. A resolved signal with nothing active to resolve: `Ignore`.
//! 5. Otherwise: `New`.

use domovoi_core::{Incident, IncidentId, Signal, SignalStatus};
use jiff::Timestamp;

use crate::policy::EnginePolicy;

/// Where a signal goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Into this active incident.
    Existing(IncidentId),
    /// Reopens this resolved incident.
    Reopen(IncidentId),
    /// Starts a new incident.
    New,
    /// Nowhere: a resolved signal with nothing to resolve.
    Ignore,
}

/// The incidents a signal could belong to.
#[derive(Debug, Clone, Copy)]
pub struct Candidates<'a> {
    /// Every active incident.
    pub active: &'a [Incident],
    /// The most recently resolved incident with the signal's fingerprint, if any.
    pub recent_resolved: Option<&'a Incident>,
}

/// Picks the incident for `signal` at `now`.
#[must_use]
pub fn correlate(
    signal: &Signal,
    candidates: &Candidates<'_>,
    policy: &EnginePolicy,
    now: Timestamp,
) -> Target {
    let fingerprint = signal.fingerprint();
    if let Some(incident) = candidates.active.iter().find(|i| i.is_member(&fingerprint)) {
        return Target::Existing(incident.id());
    }
    let firing = signal.status() == SignalStatus::Firing;
    if firing
        && let Some(resolved) = candidates.recent_resolved
        && resolved.fingerprint() == fingerprint
        && resolved
            .resolved_at()
            .is_some_and(|at| now.duration_since(at) <= policy.incident.reopen_window)
    {
        return Target::Reopen(resolved.id());
    }
    if firing && let Some(incident) = group_with(signal, candidates.active, policy, now) {
        return Target::Existing(incident.id());
    }
    if firing { Target::New } else { Target::Ignore }
}

/// The active incident a firing `signal` from another source joins, newest first.
fn group_with<'a>(
    signal: &Signal,
    active: &'a [Incident],
    policy: &EnginePolicy,
    now: Timestamp,
) -> Option<&'a Incident> {
    let target = normalise_target(signal.target()?.as_str());
    active
        .iter()
        .filter(|i| i.host() == signal.host())
        .filter(|i| i.source().is_some_and(|s| s != signal.source()))
        .filter(|i| now.duration_since(i.opened_at()) <= policy.grouping_window)
        .filter(|i| {
            i.target()
                .is_some_and(|t| normalise_target(t.as_str()) == target)
        })
        .max_by_key(|i| (i.opened_at(), i.id()))
}

/// The name of the thing a target points at, so different sources agree on it:
/// lowercased, with any URL scheme, credentials, port, path and domain removed, then any
/// replica suffix (`-1`, `_1`) and compose v1 project prefix (`project_service_1`).
///
/// `plex`, `plex-1`, `media_plex_1` and `https://plex.example:32400` are all `plex`.
#[must_use]
pub fn normalise_target(target: &str) -> String {
    let mut name = target.trim().to_lowercase();
    if let Some((_, rest)) = name.split_once("://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = authority.rsplit('@').next().unwrap_or_default();
        name = strip_port(host).to_owned();
        if !is_ip(&name) {
            name = name.split('.').next().unwrap_or_default().to_owned();
        }
    }
    let had_replica = strip_replica(&mut name);
    // Compose v1 names containers `<project>_<service>_<n>`; v2 uses hyphens, which
    // cannot be told apart from a hyphenated service name, so only underscores count.
    if had_replica && let Some((_, service)) = name.split_once('_') {
        name = service.to_owned();
    }
    name
}

fn strip_port(host: &str) -> &str {
    if host.starts_with('[') {
        return host
            .split_once(']')
            .map_or(host, |(ip, _)| ip.trim_start_matches('['));
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

fn is_ip(name: &str) -> bool {
    name.parse::<core::net::IpAddr>().is_ok()
}

/// Removes a trailing `-<digits>` or `_<digits>`; returns whether there was one.
fn strip_replica(name: &mut String) -> bool {
    let Some(pos) = name.rfind(['-', '_']) else {
        return false;
    };
    let digits = &name[pos + 1..];
    if pos == 0 || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    name.truncate(pos);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalised_target_table() {
        let cases = [
            ("plex", "plex"),
            ("plex-1", "plex"),
            ("media_plex_1", "plex"),
            ("https://plex.example:32400", "plex"),
            ("http://admin:pw@plex.example/web/index.html", "plex"),
            ("Plex", "plex"),
            ("plex_2", "plex"),
            ("home-assistant", "home-assistant"),
            ("home-assistant-1", "home-assistant"),
            ("http://192.168.1.10:8080/", "192.168.1.10"),
            ("https://[fd00::1]:8443", "fd00::1"),
            ("/dev/sda", "/dev/sda"),
            ("tcp/22", "tcp/22"),
            ("-1", "-1"),
        ];
        for (input, expected) in cases {
            assert_eq!(normalise_target(input), expected, "{input}");
        }
    }
}
