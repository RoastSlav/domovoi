use jiff::Timestamp;

use super::*;
use crate::severity::Severity;
use crate::signal::{Evidence, SourceKind, TargetKey};

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + secs).unwrap()
}

/// A successful SSH login by `user` from `ip`, firing on `host`.
fn ssh_login(host: HostId, user: &str, ip: &str, network: &str) -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert("ip", EvidenceValue::Text(ip.to_owned()));
    evidence.insert("network", EvidenceValue::Text(network.to_owned()));
    evidence.insert("count", EvidenceValue::Int(1));
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse("security.ssh_login").unwrap(),
        Some(TargetKey::parse(user).unwrap()),
        Severity::Warning,
        format!("ssh login by {user} from {ip}"),
        at(0),
    )
    .with_evidence(evidence)
}

/// A rule expecting `user`, in range `cidr`, on `host`, forever.
fn ssh_rule(host: HostId, user: &str, cidr: &str) -> AckRule {
    AckRule {
        id: AckRuleId::new(),
        host: Some(host),
        check: CheckKey::parse("security.ssh_login").unwrap(),
        target_pattern: Some(TargetPattern::parse(user).unwrap()),
        predicates: vec![EvidencePredicate::InNetwork {
            key: "ip".to_owned(),
            cidr: cidr.to_owned(),
        }],
        note: "expected SSH login".to_owned(),
        expires_at: None,
        created_at: None,
        created_by: None,
    }
}

#[test]
fn a_login_from_an_expected_address_range_is_known() {
    let nas = HostId::new();
    let alice_lan = ssh_login(nas, "alice", "192.168.1.20", "lan");
    let alice_wan = ssh_login(nas, "alice", "203.0.113.9", "wan");
    let bob_lan = ssh_login(nas, "bob", "192.168.1.20", "lan");

    let rule = ssh_rule(nas, "alice", "192.168.1.0/24");
    assert!(
        rule.matches(&alice_lan, at(10)),
        "the expected login is known"
    );
    assert!(
        !rule.matches(&alice_wan, at(10)),
        "the same user from outside the range is not expected"
    );
    assert!(
        !rule.matches(&bob_lan, at(10)),
        "another user inside the range is not expected"
    );

    let other_host = ssh_login(HostId::new(), "alice", "192.168.1.20", "lan");
    assert!(
        !rule.matches(&other_host, at(10)),
        "the rule is for one host"
    );
}
