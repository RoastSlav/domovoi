use jiff::Timestamp;

use super::*;
use crate::severity::Severity;
use crate::signal::{Evidence, SourceKind, TargetKey};

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + secs).unwrap()
}

fn blind(host: HostId, check: &str) -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert("reason", EvidenceValue::Text("cannot read".to_owned()));
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).unwrap(),
        Some(TargetKey::parse(BLIND_TARGET).unwrap()),
        Severity::Warning,
        "cannot read the journal",
        at(0),
    )
    .with_evidence(evidence)
}

fn rule_for(host: HostId, check: &str, pattern: Option<&str>) -> AckRule {
    AckRule {
        id: AckRuleId::new(),
        host: Some(host),
        check: CheckKey::parse(check).unwrap(),
        target_pattern: pattern.map(|p| TargetPattern::parse(p).unwrap()),
        predicates: Vec::new(),
        note: "known".to_owned(),
        expires_at: None,
        created_at: None,
        created_by: None,
    }
}

#[test]
fn blindness_is_covered_only_by_a_rule_that_names_it() {
    let host = HostId::new();
    let check = "security.sudo_failure";
    let signal = blind(host, check);
    let key = CheckKey::parse(check).unwrap();
    for pattern in [
        None,
        Some("*"),
        Some("journal*"),
        Some("*unreadable"),
        Some("kuker"),
    ] {
        let rule = rule_for(host, check, pattern);
        assert!(
            !rule.matches(&signal, at(1)),
            "{pattern:?} must not cover blindness"
        );
        assert!(
            !rule.covers_incident(host, &key, Some(BLIND_TARGET), at(1)),
            "{pattern:?} must not cover the blind incident"
        );
    }
    let named = rule_for(host, check, Some(BLIND_TARGET));
    assert!(named.matches(&signal, at(1)));
    assert!(named.covers_incident(host, &key, Some(BLIND_TARGET), at(1)));
    // A target-less rule still covers the events themselves.
    let wide = rule_for(host, check, None);
    assert!(wide.covers_incident(host, &key, Some("kuker"), at(1)));
}
