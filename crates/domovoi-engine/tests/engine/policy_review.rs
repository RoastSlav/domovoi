//! Edge cases found in review: time-of-day and TTL validation, target intersection,
//! equal-scope resolution, lock findings and the trace of several overridden denies.

use std::collections::BTreeMap;

use domovoi_core::capability::EffectClass;
use domovoi_engine::policy::{
    Condition, FindingKind, Outcome::Allow, Outcome::Deny, Outcome::RequireApproval, OutcomeReason,
    Overridden, PolicyError, Rule, TargetScope, TimeOfDay, Warning, evaluate, validate_publication,
    validate_rules,
};
use jiff::SignedDuration;
use proptest::prelude::*;

use crate::policy_fixtures::*;

fn window(start: (u8, u8), end: (u8, u8)) -> Condition {
    Condition::TimeWindow {
        start: TimeOfDay {
            hour: start.0,
            minute: start.1,
        },
        end: TimeOfDay {
            hour: end.0,
            minute: end.1,
        },
    }
}

#[test]
fn time_of_day_deserialization_rejects_out_of_range_values() {
    for (hour, minute) in [(24, 0), (0, 60), (255, 255)] {
        let json = serde_json::json!({"hour": hour, "minute": minute});
        assert!(serde_json::from_value::<TimeOfDay>(json).is_err());
    }
    let ok = serde_json::json!({"hour": 23, "minute": 59});
    assert_eq!(
        serde_json::from_value::<TimeOfDay>(ok).unwrap(),
        TimeOfDay {
            hour: 23,
            minute: 59
        }
    );
}

#[test]
fn validation_rejects_a_time_of_day_built_out_of_range() {
    for bad in [window((24, 0), (1, 0)), window((1, 0), (9, 60))] {
        let rules = [rule("a", Allow).when(bad)];
        assert!(matches!(
            validate_rules(&rules),
            Err(PolicyError::InvalidTimeOfDay { rule }) if rule == rid("a")
        ));
    }
}

#[test]
fn negative_max_ttl_is_rejected_and_zero_is_accepted() {
    let ttl = |secs: i64| Condition::MaxTtl {
        max_ttl: SignedDuration::from_secs(secs),
    };
    let rules = [rule("a", Allow).when(ttl(-1))];
    assert!(matches!(
        validate_rules(&rules),
        Err(PolicyError::NegativeMaxTtl { rule }) if rule == rid("a")
    ));
    assert!(validate_rules(&[rule("a", Allow).when(ttl(0))]).is_ok());
}

#[test]
fn any_is_the_identity_of_target_intersection() {
    let fx = Fx::new();
    let rules = [
        rule("a", Allow).contract(RESTART),
        rule("b", Allow)
            .on_host(fx.host)
            .category(LIFECYCLE)
            .named(&["plex", "radarr"]),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.winners, ids(&["a", "b"]));
    assert_eq!(d.outcome, Allow);
    assert_eq!(d.constraints.targets, Some(names(&["plex", "radarr"])));

    let rules = [
        rule("a", Allow).contract(RESTART),
        rule("b", Allow).on_host(fx.host).category(LIFECYCLE),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.winners, ids(&["a", "b"]));
    assert_eq!(d.constraints.targets, None);
}

#[test]
fn equal_scope_disagreements_deny_under_permutation() {
    let fx = Fx::new();
    let pairs = [
        (Allow, RequireApproval),
        (Allow, Deny),
        (RequireApproval, Deny),
    ];
    for (first, second) in pairs {
        let base = [
            rule("a", first).contract(RESTART),
            rule("b", second).contract(RESTART),
        ];
        let rev = [base[1].clone(), base[0].clone()];
        let d = eval(&base, &fx.req(RESTART));
        assert_eq!(d.outcome, Deny);
        assert_eq!(d.reason, OutcomeReason::EqualScopeDeny);
        assert_eq!(d, eval(&rev, &fx.req(RESTART)));
    }
}

#[test]
fn lock_finding_requires_a_locked_rule_without_conditions() {
    let fx = Fx::new();
    let plain = [
        rule("a", Deny).contract(RESTART).locked(),
        rule("b", Allow).contract(RESTART).named(&["plex"]),
    ];
    let report = validate_publication(&plain, &fx.index, &BTreeMap::new()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == FindingKind::IneffectiveUnderLock)
    );

    let conditional = [
        rule("a", Deny)
            .contract(RESTART)
            .locked()
            .when(Condition::MaxRate { max_per_hour: 3 }),
        rule("b", Allow).contract(RESTART).named(&["plex"]),
    ];
    let report = validate_publication(&conditional, &fx.index, &BTreeMap::new()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.kind != FindingKind::IneffectiveUnderLock)
    );
}

#[test]
fn every_broader_deny_is_traced_under_one_narrower_allow() {
    let fx = Fx::new();
    let rules = [
        rule("allow", Allow)
            .on_host(fx.host)
            .contract(RESTART)
            .named(&["plex"]),
        rule("deny1", Deny).category(LIFECYCLE),
        rule("deny2", Deny).on_host(fx.host),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.winners, ids(&["allow"]));
    assert_eq!(d.outcome, Allow);
    let over = |b: &str| Overridden {
        broader: rid(b),
        by: rid("allow"),
    };
    assert_eq!(d.overridden, vec![over("deny1"), over("deny2")]);
    let danger = |deny: &str| Warning::DangerousException {
        allow: rid("allow"),
        deny: rid(deny),
    };
    assert_eq!(d.warnings, vec![danger("deny1"), danger("deny2")]);
}

fn allow_rule(n: usize, spec: (u8, u8, u8), fx: &Fx) -> Rule {
    let (host, capability, targets) = spec;
    let mut r = rule(&format!("r{n}"), Allow);
    r = match host {
        0 => r,
        1 => r.on_host(fx.host),
        _ => r.on_host(fx.other),
    };
    r = match capability {
        0 => r,
        1 => r.contract(RESTART),
        2 => r.category(LIFECYCLE),
        _ => r.effect(EffectClass::Mutate),
    };
    r.targets = match targets {
        0 => TargetScope::Any,
        1 => TargetScope::Exact(names(&["plex"])),
        2 => TargetScope::Exact(names(&["plex", "radarr"])),
        3 => TargetScope::Exact(names(&["plex", "sonarr"])),
        _ => TargetScope::Exact(names(&["radarr"])),
    };
    r
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn allow_targets_are_a_subset_of_every_winning_exact_set(
        specs in proptest::collection::vec((0u8..3, 0u8..4, 0u8..5), 1..6),
    ) {
        let fx = Fx::new();
        let rules: Vec<Rule> = specs
            .iter()
            .enumerate()
            .map(|(n, spec)| allow_rule(n, *spec, &fx))
            .collect();
        let request = fx.req(RESTART);
        let d = evaluate(&rules, &request).unwrap();
        if d.outcome == Allow {
            let winners: Vec<&Rule> = d
                .winners
                .iter()
                .map(|id| rules.iter().find(|r| &r.id == id).unwrap())
                .collect();
            match &d.constraints.targets {
                Some(targets) => {
                    prop_assert!(targets.contains(&request.target));
                    for r in &winners {
                        if let TargetScope::Exact(set) = &r.targets {
                            prop_assert!(targets.is_subset(set));
                        }
                    }
                }
                None => {
                    for r in &winners {
                        prop_assert!(matches!(r.targets, TargetScope::Any));
                    }
                }
            }
        }
    }
}
