//! The signal for a check that cannot read the journal it watches, and its recovery.

use std::collections::BTreeSet;

use domovoi_checks::framework::Check;
use domovoi_core::{Evidence, EvidenceValue, HostId, Severity, Signal, SourceKind, TargetKey};
use jiff::Timestamp;

/// The target of the signal that says a check cannot read the journal it watches. The colon
/// keeps it apart from user, unit and pattern names.
pub const BLIND_TARGET: &str = "journal:unreadable";

/// The signal for a check that could not read its journal: its events are not being watched,
/// and an unwatched log must never look like a quiet one.
#[must_use]
pub fn blind_signal(host: HostId, check: &dyn Check, why: &str, now: Timestamp) -> Signal {
    let key = check.key();
    let summary = format!("{key} cannot read the journal, so its events are not being seen");
    let target = TargetKey::parse(BLIND_TARGET).ok();
    let mut evidence = Evidence::new();
    evidence.insert("reason", EvidenceValue::Text(why.to_owned()));
    Signal::firing(
        SourceKind::Agent,
        host,
        key,
        target,
        Severity::Warning,
        &summary,
        now,
    )
    .with_class(check.class())
    .with_evidence(evidence)
}

/// The recovery to send when a run that produced a report follows a blind one. It is sent for
/// an event check too: what ends here is the blindness, not an event.
pub(super) fn ended(
    host: HostId,
    check: &dyn Check,
    previous: &BTreeSet<String>,
    now: Timestamp,
) -> Option<Signal> {
    if !previous.contains(BLIND_TARGET) {
        return None;
    }
    let key = check.key();
    let summary = format!("{key} can read the journal again");
    let target = TargetKey::parse(BLIND_TARGET).ok();
    let signal = Signal::resolved(SourceKind::Agent, host, key, target, &summary, now);
    Some(signal.with_class(check.class()))
}
