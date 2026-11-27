//! Expected events are history, not incidents.
//!
//! A firing signal of an event check (a new listener, an SSH login, a failed sudo) that an
//! unexpired known condition covers is stored with the rule's id and goes no further: no
//! incident, no event, no notification, no triage. An incident that already exists is left
//! to a person: the caller only asks for a signal that would open or reopen one. Pure.
//!
//! The reverse case is `security.listener_gone`: an agent says a listener it saw is gone. That
//! matters only when a person said the listener was expected, so an incident opens only when
//! a known condition for `security.new_port` names that exact listener on that host. Without
//! one the signal is stored, with the reason, and goes no further.

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

/// The check an agent reports a vanished listener with.
const GONE_CHECK: &str = "security.listener_gone";
/// The check whose known conditions say a listener is expected.
const LISTENER_CHECK: &str = "security.new_port";
/// The evidence key that names why a gone listener opened no incident.
pub(super) const NOT_RAISED_KEY: &str = "not_raised";
/// The evidence key that names the known condition that made a gone listener matter.
pub(super) const EXPECTED_BY_KEY: &str = "expected_by";

/// What to do with a signal of the gone-listener check.
pub(super) enum Gone {
    /// Not a gone-listener signal, or not one that would open an incident: handle as usual.
    NotApplicable,
    /// A person expected this listener: the signal, marked with the entry, opens an incident.
    Expected(Signal),
    /// Nobody expected this listener: the signal, marked with why, is only stored.
    Unexpected(Signal),
}

/// Judges a firing `security.listener_gone` signal against the expected-listener entries: a
/// host-scoped, unexpired `security.new_port` rule whose target is exactly the listener.
pub(super) fn gone_listener(acks: &[AckRule], signal: &Signal, now: Timestamp) -> Gone {
    if signal.status() != SignalStatus::Firing || signal.check().as_str() != GONE_CHECK {
        return Gone::NotApplicable;
    }
    let Some(target) = signal.target() else {
        return Gone::NotApplicable;
    };
    let entry = acks.iter().find(|rule| {
        rule.check.as_str() == LISTENER_CHECK
            && rule.host == Some(signal.host())
            && rule.expires_at.is_none_or(|at| now < at)
            && rule
                .target_pattern
                .as_ref()
                .is_some_and(|pattern| pattern.to_string() == target.as_str())
    });
    let mut evidence = signal.evidence().clone();
    match entry {
        Some(rule) => {
            evidence.insert(EXPECTED_BY_KEY, EvidenceValue::Text(rule.id.to_string()));
            Gone::Expected(signal.clone().with_evidence(evidence))
        }
        None => {
            evidence.insert(
                NOT_RAISED_KEY,
                EvidenceValue::Text(
                    "no known condition says this listener was expected, so nobody is told"
                        .to_owned(),
                ),
            );
            Gone::Unexpected(signal.clone().with_evidence(evidence))
        }
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::{
        AckRuleId, CheckKey, HostId, Severity, SourceKind, TargetKey, TargetPattern,
    };

    use super::*;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(1_700_000_000 + secs).unwrap()
    }

    fn gone(host: HostId, target: &str) -> Signal {
        Signal::firing(
            SourceKind::Agent,
            host,
            CheckKey::parse(GONE_CHECK).unwrap(),
            Some(TargetKey::parse(target).unwrap()),
            Severity::Warning,
            format!("expected listener {target} is gone"),
            at(0),
        )
    }

    fn entry(host: Option<HostId>, pattern: &str, expires: Option<Timestamp>) -> AckRule {
        AckRule {
            id: AckRuleId::new(),
            host,
            check: CheckKey::parse(LISTENER_CHECK).unwrap(),
            target_pattern: Some(TargetPattern::parse(pattern).unwrap()),
            predicates: Vec::new(),
            note: "ssh".to_owned(),
            expires_at: expires,
            created_at: None,
            created_by: None,
        }
    }

    #[test]
    fn a_gone_listener_matters_only_with_an_exact_host_entry() {
        let nas = HostId::new();
        let signal = gone(nas, "0.0.0.0:22/tcp");
        let kind = |acks: &[AckRule]| match gone_listener(acks, &signal, at(10)) {
            Gone::Expected(_) => "expected",
            Gone::Unexpected(_) => "unexpected",
            Gone::NotApplicable => "not applicable",
        };
        assert_eq!(kind(&[]), "unexpected", "nobody expected it");
        assert_eq!(
            kind(&[entry(Some(nas), "0.0.0.0:22/tcp", None)]),
            "expected"
        );
        assert_eq!(
            kind(&[entry(None, "0.0.0.0:22/tcp", None)]),
            "unexpected",
            "an entry for every host is not this host's listener"
        );
        assert_eq!(
            kind(&[entry(Some(nas), "0.0.0.0:*", None)]),
            "unexpected",
            "a wildcard never says one listener is expected"
        );
        assert_eq!(
            kind(&[entry(Some(HostId::new()), "0.0.0.0:22/tcp", None)]),
            "unexpected"
        );
        assert_eq!(
            kind(&[entry(Some(nas), "0.0.0.0:22/tcp", Some(at(5)))]),
            "unexpected",
            "an expired entry expects nothing"
        );
    }

    #[test]
    fn the_stored_signal_says_why_it_opened_nothing() {
        let signal = gone(HostId::new(), "0.0.0.0:22/tcp");
        let Gone::Unexpected(kept) = gone_listener(&[], &signal, at(0)) else {
            panic!("expected the unexpected branch");
        };
        assert!(matches!(
            kept.evidence().get(NOT_RAISED_KEY),
            Some(EvidenceValue::Text(t)) if t.contains("no known condition")
        ));
    }
}
