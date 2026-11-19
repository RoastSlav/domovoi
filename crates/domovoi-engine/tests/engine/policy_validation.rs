//! Malformed rule sets and requests are refused, and the serde shapes of rules.

use domovoi_core::capability::{ConstraintKind, EffectClass};
use domovoi_engine::policy::{
    CanonicalArgs, Condition, ContainmentInput, EvaluateRequest, FactState, MAX_FACTS,
    Outcome::Allow, Outcome::Deny, Outcome::RequireApproval, OutcomeReason, PolicyError, Rule,
    TimeOfDay, UnavailableReason, evaluate, evaluate_shadow, names_to_resolve, resolve_contract,
    simulate, validate_publication, validate_rules,
};
use jiff::SignedDuration;
use serde_json::json;

use crate::policy_fixtures::*;

fn at(hour: u8, minute: u8) -> TimeOfDay {
    TimeOfDay { hour, minute }
}

fn window(start: TimeOfDay, end: TimeOfDay) -> Condition {
    Condition::TimeWindow { start, end }
}

fn arg(op: ConstraintKind, value: i64) -> Condition {
    Condition::ArgConstraint {
        arg: "wait_seconds".to_owned(),
        op,
        value,
    }
}

fn arg_named(key: &str) -> Condition {
    Condition::ArgConstraint {
        arg: key.to_owned(),
        op: ConstraintKind::IntAtLeast,
        value: 0,
    }
}

#[test]
fn invalid_rule_set_never_allows() {
    let fx = Fx::new();
    let ok = rule("ok", Allow);
    let mut too_many = fx.req(RESTART);
    for n in 0..=MAX_FACTS {
        too_many
            .facts
            .insert(fact(&format!("f{n}")), FactState::Unknown);
    }
    let mut containment_on_mutate = fx.req(RESTART);
    containment_on_mutate.containment = ContainmentInput {
        attested: true,
        ..ContainmentInput::default()
    };
    let mut hash_without_ceiling = fx.req(RESTART);
    hash_without_ceiling.ceiling = None;
    hash_without_ceiling.expected_ceiling_hash = Some("h".to_owned());

    let cases: Vec<(&str, Vec<Rule>, EvaluateRequest, PolicyError)> = vec![
        (
            "locked allow",
            vec![ok.clone(), rule("bad", Allow).locked()],
            fx.req(RESTART),
            PolicyError::LockedNotDeny {
                rule: rid("bad"),
                found: "allow",
            },
        ),
        (
            "unavailable outcome",
            vec![
                ok.clone(),
                rule(
                    "bad",
                    domovoi_engine::policy::Outcome::Unavailable(UnavailableReason::SafetyUnknown),
                ),
            ],
            fx.req(RESTART),
            PolicyError::UnavailableRuleOutcome { rule: rid("bad") },
        ),
        (
            "duplicate id",
            vec![ok.clone(), rule("ok", Deny)],
            fx.req(RESTART),
            PolicyError::DuplicateRuleId(rid("ok")),
        ),
        (
            "empty target set",
            vec![ok.clone(), rule("bad", Deny).named(&[])],
            fx.req(RESTART),
            PolicyError::EmptyTargetSet { rule: rid("bad") },
        ),
        (
            "degenerate window",
            vec![
                ok.clone(),
                rule("bad", Deny).when(window(at(8, 0), at(8, 0))),
            ],
            fx.req(RESTART),
            PolicyError::DegenerateTimeWindow {
                rule: rid("bad"),
                start: "08:00".to_owned(),
                end: "08:00".to_owned(),
            },
        ),
        (
            "bad argument key",
            vec![
                ok.clone(),
                rule("bad", Deny).when(Condition::ArgConstraint {
                    arg: "Wait".to_owned(),
                    op: ConstraintKind::IntAtMost,
                    value: 1,
                }),
            ],
            fx.req(RESTART),
            PolicyError::BadArgKey {
                rule: rid("bad"),
                key: "Wait".to_owned(),
            },
        ),
        (
            "containment on a mutate contract",
            vec![ok.clone()],
            containment_on_mutate,
            PolicyError::ContainmentOnNonContain,
        ),
        (
            "ceiling hash without a ceiling",
            vec![ok.clone()],
            hash_without_ceiling,
            PolicyError::CeilingHashMissing,
        ),
        (
            "too many facts",
            vec![ok.clone()],
            too_many,
            PolicyError::TooManyFacts {
                count: MAX_FACTS + 1,
                max: MAX_FACTS,
            },
        ),
    ];
    for (label, rules, request, error) in cases {
        assert_eq!(evaluate(&rules, &request), Err(error.clone()), "{label}");
        assert_eq!(
            evaluate_shadow(&rules, &request),
            Err(error.clone()),
            "{label}"
        );
        let simulated = simulate(
            &rules,
            std::slice::from_ref(&request),
            domovoi_engine::policy::RuleSelection::Enforced,
        );
        assert_eq!(simulated, vec![Err(error)], "{label}");
    }
    let (long, longest) = ("a".repeat(65), "a".repeat(64));
    for key in ["", "1a", "a-b", "A", long.as_str()] {
        let rules = [rule("k", Deny).when(arg_named(key))];
        assert!(
            matches!(validate_rules(&rules), Err(PolicyError::BadArgKey { .. })),
            "{key:?}"
        );
    }
    for key in ["a", "wait_seconds", "a1_b2", longest.as_str()] {
        assert_eq!(
            validate_rules(&[rule("k", Deny).when(arg_named(key))]),
            Ok(()),
            "{key:?}"
        );
    }
}

#[test]
fn names_to_resolve_is_the_union_of_exact_sets() {
    let rules = [
        rule("a", Allow).named(&["plex", "radarr"]),
        rule("b", Deny).named(&["radarr", "sonarr"]),
        rule("c", Allow),
        rule("d", Allow).named(&["off"]).off(),
    ];
    assert_eq!(
        names_to_resolve(&rules),
        names(&["off", "plex", "radarr", "sonarr"]),
        "even a disabled rule's names are resolved, so toggling it needs no new lookup"
    );
    assert!(names_to_resolve(&[rule("c", Allow)]).is_empty());
}

#[test]
fn rule_serializes_with_adjacent_tags() {
    let fx = Fx::new();
    let mut rule = rule("r1", RequireApproval);
    rule.capability = domovoi_engine::policy::CapabilityScope::Contract(cid(RESTART));
    rule.targets = domovoi_engine::policy::TargetScope::Exact(names(&["plex"]));
    rule.conditions = vec![
        Condition::MaxRate { max_per_hour: 3 },
        arg(ConstraintKind::IntAtMost, 60),
        window(at(9, 5), at(17, 0)),
        Condition::RequiresFact {
            fact: fact(FACT),
            expected: true,
        },
    ];
    let value = serde_json::to_value(&rule).unwrap();
    assert_eq!(value["host"], json!("all"));
    assert_eq!(
        value["capability"],
        json!({"kind": "contract", "value": "container.restart"})
    );
    assert_eq!(
        value["targets"],
        json!({"kind": "exact", "names": ["plex"]})
    );
    assert_eq!(value["outcome"], json!({"kind": "require_approval"}));
    assert_eq!(
        value["conditions"][0],
        json!({"kind": "max_rate", "max_per_hour": 3})
    );
    assert_eq!(
        value["conditions"][1],
        json!({"kind": "arg_constraint", "arg": "wait_seconds", "op": "int_at_most", "value": 60})
    );
    assert_eq!(
        value["conditions"][2],
        json!({"kind": "time_window", "start": {"hour": 9, "minute": 5}, "end": {"hour": 17, "minute": 0}})
    );
    let back: Rule = serde_json::from_value(value).unwrap();
    assert_eq!(back, rule);
    let mut request = fx.req(RESTART);
    request.args = CanonicalArgs::from_canonical(
        "wait_seconds:2:30
",
    );
    request.facts.insert(fact(FACT), FactState::Known(true));
    let d = eval(&[back], &request);
    assert_eq!(
        (d.outcome, d.reason),
        (RequireApproval, OutcomeReason::SingleWinner)
    );
    assert_eq!(d.matched, ids(&["r1"]));

    assert_eq!(
        serde_json::to_value(Allow).unwrap(),
        json!({"kind": "allow"})
    );
    assert_eq!(
        serde_json::to_value(domovoi_engine::policy::Outcome::Unavailable(
            UnavailableReason::SafetyUnknown
        ))
        .unwrap(),
        json!({"kind": "unavailable", "reason": "safety_unknown"})
    );
    assert_eq!(serde_json::to_value(Deny).unwrap(), json!({"kind": "deny"}));
    assert_eq!(
        serde_json::to_value(FactState::Known(true)).unwrap(),
        json!({"kind": "known", "value": true})
    );
    assert_eq!(
        serde_json::to_value(FactState::Stale).unwrap(),
        json!({"kind": "stale"})
    );
    assert_eq!(
        serde_json::from_value::<FactState>(json!({"kind": "known", "value": false})).unwrap(),
        FactState::Known(false)
    );
    assert!(serde_json::from_value::<Rule>(json!({"id": "bad id"})).is_err());
}

#[test]
fn max_ttl_condition_round_trips_through_json() {
    let condition = Condition::MaxTtl {
        max_ttl: SignedDuration::from_secs(900),
    };
    let text = serde_json::to_string(&condition).unwrap();
    assert_eq!(
        serde_json::from_str::<Condition>(&text).unwrap(),
        condition,
        "{text}"
    );
}

#[test]
fn rule_outcome_unavailable_is_rejected() {
    let fx = Fx::new();
    let rules = [rule(
        "r",
        domovoi_engine::policy::Outcome::Unavailable(UnavailableReason::SafetyUnknown),
    )];
    let expected = PolicyError::UnavailableRuleOutcome { rule: rid("r") };
    assert_eq!(validate_rules(&rules), Err(expected.clone()));
    assert_eq!(evaluate(&rules, &fx.req(RESTART)), Err(expected.clone()));
    assert_eq!(
        validate_publication(&rules, &fx.index, &Default::default()),
        Err(expected)
    );
}

#[test]
fn empty_exact_target_set_is_rejected() {
    let fx = Fx::new();
    let rules = [rule("r", Allow).named(&[])];
    let expected = PolicyError::EmptyTargetSet { rule: rid("r") };
    assert_eq!(validate_rules(&rules), Err(expected.clone()));
    assert_eq!(evaluate(&rules, &fx.req(RESTART)), Err(expected.clone()));
    assert_eq!(
        validate_publication(&rules, &fx.index, &Default::default()),
        Err(expected)
    );
}

#[test]
fn unknown_contract_is_rejected_not_defaulted() {
    let fx = Fx::new();
    let unknown = cid("container.nope");
    assert_eq!(
        resolve_contract(&fx.index, &unknown),
        Err(PolicyError::UnknownContract(unknown.clone()))
    );
    let meta = resolve_contract(&fx.index, &cid(RESTART)).unwrap();
    assert_eq!(meta.effect, EffectClass::Mutate);
    assert_eq!(meta.category, cat(LIFECYCLE));

    let rules = [rule("r", Allow).contract("container.nope")];
    assert_eq!(
        validate_publication(&rules, &fx.index, &Default::default()),
        Err(PolicyError::UnknownContract(unknown))
    );
}

#[test]
fn category_absent_from_index_is_rejected() {
    let fx = Fx::new();
    let rules = [rule("r", Allow).category("no.such")];
    assert_eq!(
        validate_publication(&rules, &fx.index, &Default::default()),
        Err(PolicyError::CategoryNotInIndex(cat("no.such")))
    );
    assert!(
        validate_publication(
            &[rule("r", Allow).category(LIFECYCLE)],
            &fx.index,
            &Default::default()
        )
        .is_ok()
    );
}
