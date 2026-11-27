//! Expected events are history, not incidents.
//!
//! A firing signal of an event check (a new listener, an SSH login, a failed sudo) that an
//! unexpired known condition covers is stored with the rule's id and goes no further: no
//! incident, no event, no notification, no triage. An incident that already exists is left
//! to a person: the caller only asks for a signal that would open or reopen one. Pure.

use domovoi_core::{
    AckRule, EvidenceValue, MATCHED_RULE_KEY, Signal, SignalStatus, is_expected_history_check,
};
use jiff::Timestamp;

/// The signal to store, with the id of the rule that explains it, when `acks` explain
/// `signal` at `now`; `None` when the signal goes on to the incidents as usual.
pub(super) fn explained(acks: &[AckRule], signal: &Signal, now: Timestamp) -> Option<Signal> {
    if signal.status() != SignalStatus::Firing || !is_expected_history_check(signal.check()) {
        return None;
    }
    let rule = acks.iter().find(|rule| rule.matches(signal, now))?;
    let mut evidence = signal.evidence().clone();
    evidence.insert(MATCHED_RULE_KEY, EvidenceValue::Text(rule.id.to_string()));
    Some(signal.clone().with_evidence(evidence))
}
