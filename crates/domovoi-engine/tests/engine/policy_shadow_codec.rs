//! The stored rule envelope, the rule-set hash and the decision document.

use domovoi_core::capability::{ConstraintKind, FactId};
use domovoi_engine::policy::{Condition, Outcome, Rule, TimeOfDay, evaluate};
use domovoi_engine::policy_shadow::{
    CodecError, DecisionDoc, DecisionEnvelope, decode_rule, decode_rules, encode_rule, rules_hash,
    stored_rules,
};
use jiff::SignedDuration;
use sha2::{Digest, Sha256};

use crate::policy_fixtures::{Fx, GATED, RESTART, cid, fact, ident, name, rid};
use crate::policy_shadow_support::{named_rule, rule, shadow_rule};

fn conditions() -> Vec<(&'static str, Condition)> {
    vec![
        (
            "arg_constraint",
            Condition::ArgConstraint {
                arg: "count".to_owned(),
                op: ConstraintKind::IntAtLeast,
                value: 3,
            },
        ),
        (
            "time_window",
            Condition::TimeWindow {
                start: TimeOfDay {
                    hour: 22,
                    minute: 0,
                },
                end: TimeOfDay { hour: 6, minute: 0 },
            },
        ),
        (
            "requires_fact",
            Condition::RequiresFact {
                fact: FactId::parse("host.not_in_maintenance").unwrap(),
                expected: true,
            },
        ),
        ("max_rate", Condition::MaxRate { max_per_hour: 3 }),
        (
            "max_ttl",
            Condition::MaxTtl {
                max_ttl: SignedDuration::from_secs(600),
            },
        ),
    ]
}

fn with(condition: Condition) -> Rule {
    Rule {
        conditions: vec![condition],
        ..rule("cond", Outcome::Allow)
    }
}

#[test]
fn rule_envelope_matches_golden() {
    let mut lines = Vec::new();
    for (label, condition) in conditions() {
        lines.push(format!("{label}: {}", encode_rule(&with(condition))));
    }
    let locked = Rule {
        locked: true,
        note: "never prune".to_owned(),
        ..named_rule("no-prune", "nas", Outcome::Deny)
    };
    lines.push(format!("locked_deny: {}", encode_rule(&locked)));
    lines.push(format!(
        "shadow_allow: {}",
        encode_rule(&shadow_rule("try-allow", Outcome::Allow))
    ));
    insta::assert_snapshot!(lines.join("\n"));
}

#[test]
fn rule_envelope_round_trips_every_condition() {
    for (label, condition) in conditions() {
        let original = with(condition);
        let back = decode_rule(&encode_rule(&original)).unwrap();
        assert_eq!(back, original, "{label}");
    }
    let good = encode_rule(&rule("a", Outcome::Allow));
    assert!(matches!(
        decode_rule(&good.replace("\"v\":1", "\"v\":2")),
        Err(CodecError::Version(2))
    ));
    let extra = good.replacen('{', "{\"extra\":1,", 1);
    assert!(matches!(decode_rule(&extra), Err(CodecError::Json(_))));
    assert!(matches!(
        decode_rule("{\"v\":1,\"rule\":{\"id\":\"a\"}}"),
        Err(CodecError::Json(_))
    ));
}

#[test]
fn rules_hash_is_order_and_content_sensitive() {
    let a = rule("a", Outcome::Allow);
    let b = rule("b", Outcome::Deny);
    let forward = rules_hash(&stored_rules(&[a.clone(), b.clone()]));
    let reversed = rules_hash(&stored_rules(&[b.clone(), a.clone()]));
    assert_ne!(forward, reversed);
    let noted = Rule {
        note: "changed".to_owned(),
        ..a.clone()
    };
    assert_ne!(forward, rules_hash(&stored_rules(&[noted, b])));
    assert_eq!(forward.len(), 64);
    assert_eq!(
        rules_hash(&[]),
        hex::encode(Sha256::digest(b"policy-rules-v1\n"))
    );
}

#[test]
fn stored_rules_decode_in_ordinal_order_and_check_projections() {
    let rules = vec![rule("a", Outcome::Allow), shadow_rule("b", Outcome::Deny)];
    let mut stored = stored_rules(&rules);
    assert_eq!(decode_rules(&stored).unwrap(), rules);
    stored[1].shadow = false;
    assert!(matches!(
        decode_rules(&stored),
        Err(CodecError::ProjectionMismatch { .. })
    ));
}

#[test]
fn decision_doc_mirrors_decision_field_for_field() {
    let fx = Fx::new();
    // A specific allow beats a broad deny, with every kind of constraint, and consults a fact.
    let mut request = fx.req(GATED);
    request.facts.insert(
        fact("host.not_in_maintenance"),
        domovoi_engine::policy::FactState::Known(true),
    );
    request.requested_rate_per_hour = Some(2);
    request.requested_ttl = Some(SignedDuration::from_secs(300));
    request
        .rule_targets
        .insert(name("plex"), ident("container:abc/plex"));
    let allow = Rule {
        capability: domovoi_engine::policy::CapabilityScope::Contract(cid(GATED)),
        targets: domovoi_engine::policy::TargetScope::Exact([name("plex")].into()),
        conditions: vec![
            Condition::MaxRate { max_per_hour: 3 },
            Condition::MaxTtl {
                max_ttl: SignedDuration::from_secs(600),
            },
            Condition::RequiresFact {
                fact: fact("host.not_in_maintenance"),
                expected: true,
            },
        ],
        ..rule("specific-allow", Outcome::Allow)
    };
    let broad = rule("broad-deny", Outcome::Deny);
    let decision = evaluate(&[allow, broad], &request).unwrap();
    let doc = DecisionDoc::from_decision(&decision);
    assert_eq!(doc.matched, decision.matched);
    assert_eq!(
        doc.constraints.max_per_hour,
        decision.constraints.max_per_hour
    );
    assert_eq!(doc.facts_used.len(), decision.facts_used.len());
    assert!(!doc.overridden.is_empty());
    insta::assert_snapshot!(
        "decision_doc_with_constraints",
        serde_json::to_string_pretty(&doc).unwrap()
    );

    // A locked deny always carries the ordinary resolution it overrode.
    let locked = Rule {
        locked: true,
        ..rule("lock", Outcome::Deny)
    };
    let all_allow = Rule {
        capability: domovoi_engine::policy::CapabilityScope::Contract(cid(RESTART)),
        ..rule("allow", Outcome::Allow)
    };
    let decision = evaluate(&[locked, all_allow], &fx.req(RESTART)).unwrap();
    let doc = DecisionDoc::from_decision(&decision);
    assert!(doc.would_have_been.is_some());
    assert_eq!(doc.locked, vec![rid("lock")]);
    insta::assert_snapshot!(
        "decision_doc_locked_deny",
        serde_json::to_string_pretty(&doc).unwrap()
    );
    let envelope = DecisionEnvelope::Decision(Box::new(doc));
    assert_eq!(
        DecisionEnvelope::from_json(&envelope.to_json().unwrap()),
        Some(envelope)
    );
}

#[test]
fn error_envelope_keeps_only_the_code() {
    let duplicate = vec![rule("a", Outcome::Allow), rule("a", Outcome::Deny)];
    let fx = Fx::new();
    let err = evaluate(&duplicate, &fx.req(RESTART)).unwrap_err();
    let envelope = DecisionEnvelope::from_result(&Err(err));
    assert_eq!(
        envelope.to_json().unwrap(),
        r#"{"v":1,"error":"duplicate_rule_id"}"#
    );
    assert_eq!(envelope.outcome_text(), "error");
}
