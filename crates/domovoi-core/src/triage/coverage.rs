//! Which security events a stored triage report has not seen.
//!
//! A verdict speaks only for what had happened when its run read the incident. Every later
//! event is written to the timeline with its evidence, and counted here, so a page or a card
//! can say that the verdict does not cover it.

use crate::ports::IncidentTimelineEntry;
use crate::signal::{CheckKey, Signal};

use super::security::current_episode;

/// The timeline kind of one security event that joined an incident.
pub const SECURITY_EVENT_KIND: &str = "security.event";

/// The detail key of a stored report that says what time it saw the incident at.
pub const SEEN_UNTIL_KEY: &str = "seen_until";

/// Whether each firing of `check` is a new security event, and not a state it repeats on
/// every run, as an open listener is.
#[must_use]
pub fn reports_security_events(check: &CheckKey) -> bool {
    matches!(
        check.as_str(),
        "security.ssh_login" | "security.sudo_failure" | "security.listener_gone"
    )
}

/// Whether `signal` reports a security event that happened. A blind check reports none.
#[must_use]
pub fn is_security_event(signal: &Signal) -> bool {
    reports_security_events(signal.check())
        && signal
            .target()
            .is_none_or(|target| target.as_str() != crate::blind::BLIND_TARGET)
}

/// The security events of the current episode that came after the latest report saw the
/// incident, oldest first. Empty without a report: there is no verdict to qualify.
#[must_use]
pub fn uncovered_events<'a>(
    check: &CheckKey,
    entries: &'a [IncidentTimelineEntry],
) -> Vec<&'a IncidentTimelineEntry> {
    let current = current_episode(check, entries);
    let Some(report) = current
        .iter()
        .rev()
        .find(|entry| matches!(entry.kind.as_str(), "triage.report" | "triage.cached"))
    else {
        return Vec::new();
    };
    // Reports stored without the time saw nothing later than the moment they were attached.
    let seen = report.detail[SEEN_UNTIL_KEY]
        .as_str()
        .and_then(|at| at.parse().ok())
        .unwrap_or(report.at);
    current
        .iter()
        .filter(|entry| entry.kind == SECURITY_EVENT_KIND && entry.at > seen)
        .collect()
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
