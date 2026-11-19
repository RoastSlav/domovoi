//! Rule resolution: specificity, conflicts, locks, shadows and the effect defaults.

use domovoi_core::capability::EffectClass;
use domovoi_engine::policy::{
    Condition, ConfirmationRequirement, Constraints, Outcome, Outcome::Allow, Outcome::Deny,
    Outcome::RequireApproval, OutcomeReason, Overridden, PolicyError, Warning, evaluate,
    evaluate_shadow, validate_rules,
};
use jiff::SignedDuration;

use crate::policy_fixtures::*;

fn over(broader: &str, by: &str) -> Overridden {
    Overridden {
        broader: rid(broader),
        by: rid(by),
    }
}

fn danger(allow: &str, deny: &str) -> Warning {
    Warning::DangerousException {
        allow: rid(allow),
        deny: rid(deny),
    }
}

#[test]
fn specific_allow_overrides_broader_deny() {
    let fx = Fx::new();
    let rules = [
        rule("broad", Deny).contract(RESTART),
        rule("narrow", Allow)
            .on_host(fx.host)
            .contract(RESTART)
            .named(&["plex"]),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Allow);
    assert_eq!(d.reason, OutcomeReason::SingleWinner);
    assert_eq!(d.matched, ids(&["broad", "narrow"]));
    assert_eq!(d.winners, ids(&["narrow"]));
    assert_eq!(d.overridden, vec![over("broad", "narrow")]);
    assert_eq!(d.warnings, vec![danger("narrow", "broad")]);
    assert_eq!(d.constraints.targets, Some(names(&["plex"])));
    assert!(d.hard_limits_applied.is_empty());
    assert!(d.would_have_been.is_none());

    let mut radarr = fx.req(RESTART);
    radarr.target = name("radarr");
    radarr.target_identity = ident("container:def/radarr");
    let mut elsewhere = fx.req(RESTART);
    elsewhere.host = fx.other;
    for (label, request) in [("other target", radarr), ("other host", elsewhere)] {
        let d = eval(&rules, &request);
        assert_eq!(d.outcome, Deny, "{label}");
        assert_eq!(d.matched, ids(&["broad"]), "{label}");
        assert_eq!(d.winners, ids(&["broad"]), "{label}");
        assert!(d.overridden.is_empty(), "{label}");
        assert!(d.warnings.is_empty(), "{label}");
    }
}

#[test]
fn broader_deny_applies_without_exception() {
    let fx = Fx::new();
    let rules = [rule("broad", Deny).category(LIFECYCLE)];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Deny);
    assert_eq!(d.reason, OutcomeReason::SingleWinner);
    assert_eq!(d.winners, ids(&["broad"]));
    assert!(d.overridden.is_empty());
    assert!(d.warnings.is_empty());
    assert_eq!(d.constraints, Constraints::default());
}

#[test]
fn equal_scope_deny_wins() {
    let fx = Fx::new();
    for other in [Allow, RequireApproval] {
        let rules = [
            rule("a", other).contract(RESTART),
            rule("b", Deny).contract(RESTART),
        ];
        let d = eval(&rules, &fx.req(RESTART));
        assert_eq!(d.outcome, Deny, "{other:?}");
        assert_eq!(d.reason, OutcomeReason::EqualScopeDeny, "{other:?}");
        assert_eq!(d.winners, ids(&["a", "b"]), "{other:?}");
        assert!(d.overridden.is_empty(), "{other:?}");
    }
    let rules = [
        rule("a", Allow).contract(RESTART),
        rule("b", RequireApproval).contract(RESTART),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(
        (d.outcome, d.reason),
        (Deny, OutcomeReason::EqualScopeDeny),
        "equal scope never picks the softer outcome"
    );
}

fn incomparable_pair(fx: &Fx, second: Outcome) -> [domovoi_engine::policy::Rule; 2] {
    [
        rule("wide_host", Allow).contract(RESTART).named(&["plex"]),
        rule("wide_cap", second)
            .on_host(fx.host)
            .category(LIFECYCLE),
    ]
}

#[test]
fn incomparable_conflict_falls_back_to_deny() {
    let fx = Fx::new();
    let d = eval(&incomparable_pair(&fx, Deny), &fx.req(RESTART));
    assert_eq!(d.outcome, Deny);
    assert_eq!(d.reason, OutcomeReason::IncomparableFallback);
    assert_eq!(d.winners, ids(&["wide_cap", "wide_host"]));
    assert!(d.overridden.is_empty());
}

#[test]
fn incomparable_allow_vs_approval_requires_approval() {
    let fx = Fx::new();
    let d = eval(&incomparable_pair(&fx, RequireApproval), &fx.req(RESTART));
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(d.reason, OutcomeReason::IncomparableFallback);
    assert_eq!(d.winners, ids(&["wide_cap", "wide_host"]));
    assert_eq!(d.constraints, Constraints::default());
    assert!(d.hard_limits_applied.is_empty());
}

#[test]
fn equal_scope_allows_intersect_constraints() {
    let fx = Fx::new();
    let rules = [
        rule("a", Allow)
            .contract(RESTART)
            .when(Condition::MaxRate { max_per_hour: 5 })
            .when(Condition::MaxTtl {
                max_ttl: SignedDuration::from_secs(600),
            }),
        rule("b", Allow)
            .contract(RESTART)
            .when(Condition::MaxRate { max_per_hour: 3 })
            .when(Condition::MaxTtl {
                max_ttl: SignedDuration::from_secs(900),
            }),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Allow);
    assert_eq!(d.winners, ids(&["a", "b"]));
    assert_eq!(d.constraints.max_per_hour, Some(3));
    assert_eq!(d.constraints.max_ttl, Some(SignedDuration::from_secs(600)));
    assert_eq!(d.constraints.targets, None);

    let mut asked = fx.req(RESTART);
    asked.requested_rate_per_hour = Some(2);
    asked.requested_ttl = Some(SignedDuration::from_secs(60));
    let d = eval(&rules, &asked);
    assert_eq!(d.constraints.max_per_hour, Some(2));
    assert_eq!(d.constraints.max_ttl, Some(SignedDuration::from_secs(60)));

    let mut too_fast = fx.req(RESTART);
    too_fast.requested_rate_per_hour = Some(4);
    let d = eval(&rules, &too_fast);
    assert_eq!(
        d.matched,
        ids(&["a"]),
        "rule b's bound is below the request"
    );
    assert_eq!(d.constraints.max_per_hour, Some(4));
}

#[test]
fn equal_scope_allows_intersect_target_sets() {
    let fx = Fx::new();
    let rules = [
        rule("a", Allow)
            .contract(RESTART)
            .named(&["plex", "radarr"]),
        rule("b", Allow)
            .contract(RESTART)
            .named(&["plex", "sonarr"]),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Allow);
    assert_eq!(d.reason, OutcomeReason::SingleWinner);
    assert_eq!(d.winners, ids(&["a", "b"]));
    assert_eq!(d.constraints.targets, Some(names(&["plex"])));

    let mut with_any = rules.to_vec();
    with_any.push(rule("c", Allow).contract(RESTART));
    let d = eval(&with_any, &fx.req(RESTART));
    assert_eq!(
        d.winners,
        ids(&["a", "b"]),
        "the Any rule is broader and loses"
    );
    assert_eq!(d.constraints.targets, Some(names(&["plex"])));
}

#[test]
fn equal_scope_allows_conjoin_conditions() {
    let fx = Fx::new();
    let rate = Condition::MaxRate { max_per_hour: 5 };
    let ttl = Condition::MaxTtl {
        max_ttl: SignedDuration::from_secs(600),
    };
    let arg = Condition::ArgConstraint {
        arg: "wait_seconds".to_owned(),
        op: domovoi_core::capability::ConstraintKind::IntAtMost,
        value: 60,
    };
    let rules = [
        rule("b", Allow).contract(RESTART).when(arg.clone()),
        rule("a", Allow)
            .contract(RESTART)
            .when(rate.clone())
            .when(ttl.clone()),
    ];
    let mut request = fx.req(RESTART);
    request.args = domovoi_engine::policy::CanonicalArgs::from_canonical("wait_seconds:2:30\n");
    let d = eval(&rules, &request);
    assert_eq!(d.outcome, Allow);
    assert_eq!(
        d.constraints.conditions,
        vec![rate, ttl, arg],
        "winner id order, then each rule's own order"
    );
}

#[test]
fn no_match_uses_effect_default() {
    let fx = Fx::new();
    let table = [
        (READ, Allow, ConfirmationRequirement::None),
        (RESTART, RequireApproval, ConfirmationRequirement::None),
        (CONTAIN, RequireApproval, ConfirmationRequirement::None),
        (DESTROY, RequireApproval, ConfirmationRequirement::Typed),
    ];
    let ignored = [
        ("no rules", Vec::new()),
        (
            "disabled",
            vec![rule("off", Allow).off(), rule("off2", Deny).off()],
        ),
        (
            "other target",
            vec![rule("elsewhere", Deny).named(&["radarr"])],
        ),
    ];
    for (label, rules) in &ignored {
        for (contract, outcome, typed) in &table {
            let d = eval(rules, &fx.req(contract));
            let row = format!("{label} / {contract}");
            assert_eq!(d.outcome, *outcome, "{row}");
            assert_eq!(d.typed_confirmation, *typed, "{row}");
            assert_eq!(d.reason, OutcomeReason::Default, "{row}");
            assert!(d.matched.is_empty(), "{row}");
            assert!(d.winners.is_empty(), "{row}");
            assert!(d.hard_limits_applied.is_empty(), "{row}");
        }
    }
}

#[test]
fn empty_rule_set_uses_effect_default() {
    let fx = Fx::new();
    for effect in EffectClass::ALL {
        let contract = match effect {
            EffectClass::Read => READ,
            EffectClass::Mutate => RESTART,
            EffectClass::Contain => CONTAIN,
            EffectClass::Destructive => DESTROY,
        };
        let d = eval(&[], &fx.req(contract));
        let expected = if effect == EffectClass::Read {
            Allow
        } else {
            RequireApproval
        };
        assert_eq!(d.outcome, expected, "{effect:?}");
        assert_eq!(d.reason, OutcomeReason::Default, "{effect:?}");
    }
}

#[test]
fn locked_deny_beats_narrower_allow() {
    let fx = Fx::new();
    let rules = [
        rule("lock", Deny).locked().contract(RESTART),
        rule("narrow", Allow)
            .on_host(fx.host)
            .contract(RESTART)
            .named(&["plex"]),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Deny);
    assert_eq!(d.reason, OutcomeReason::LockedDeny);
    assert!(d.winners.is_empty());
    assert_eq!(d.locked, ids(&["lock"]));
    assert_eq!(d.matched, ids(&["lock", "narrow"]));
    assert_eq!(d.overridden, vec![over("narrow", "lock")]);
    assert!(d.hard_limits_applied.is_empty());
    assert!(d.warnings.is_empty());
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::None);
}

#[test]
fn multiple_locked_denies_one_deny_all_reasons() {
    let fx = Fx::new();
    let rules = [
        rule("lock2", Deny)
            .locked()
            .on_host(fx.host)
            .effect(EffectClass::Mutate),
        rule("lock1", Deny).locked(),
        rule("narrow", Allow)
            .on_host(fx.host)
            .contract(RESTART)
            .named(&["plex"]),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Deny);
    assert_eq!(d.reason, OutcomeReason::LockedDeny);
    assert_eq!(d.locked, ids(&["lock1", "lock2"]));
    assert_eq!(
        d.overridden,
        vec![over("narrow", "lock1"), over("narrow", "lock2")]
    );
}

#[test]
fn locked_flag_ignored_unless_deny() {
    let fx = Fx::new();
    for (outcome, found) in [(Allow, "allow"), (RequireApproval, "require_approval")] {
        let rules = [rule("bad", outcome).locked()];
        let expected = PolicyError::LockedNotDeny {
            rule: rid("bad"),
            found,
        };
        assert_eq!(validate_rules(&rules), Err(expected.clone()));
        assert_eq!(evaluate(&rules, &fx.req(RESTART)), Err(expected));
    }
    assert_eq!(validate_rules(&[rule("ok", Deny).locked()]), Ok(()));
}

#[test]
fn shadow_rules_never_enforced() {
    let fx = Fx::new();
    let request = fx.req(RESTART);
    let rules = [rule("s", Allow).contract(RESTART).shadow()];
    let enforced = eval(&rules, &request);
    assert_eq!(enforced.outcome, RequireApproval);
    assert!(enforced.matched.is_empty());
    let shadow = evaluate_shadow(&rules, &request).unwrap();
    assert_eq!(shadow.outcome, Allow);
    assert_eq!(shadow.matched, ids(&["s"]));

    let mixed = [
        rule("shadow_deny", Deny).shadow(),
        rule("live_allow", Allow).contract(RESTART),
    ];
    assert_eq!(eval(&mixed, &request).outcome, Allow);
    assert_eq!(eval(&mixed, &request).matched, ids(&["live_allow"]));
    let shadow = evaluate_shadow(&mixed, &request).unwrap();
    assert_eq!(shadow.outcome, Deny);
    assert_eq!(shadow.matched, ids(&["shadow_deny"]));

    let locked_shadow = [rule("lock", Deny).locked().shadow(), rule("a", Allow)];
    assert_eq!(eval(&locked_shadow, &request).outcome, Allow);
}

#[test]
fn empty_target_intersection_is_unrepresentable() {
    let fx = Fx::new();
    let rules = [
        rule("left", Allow).contract(RESTART).named(&["a", "b"]),
        rule("right", Allow).contract(RESTART).named(&["c", "d"]),
    ];
    for target in ["a", "b", "c", "d"] {
        let mut request = fx.req(RESTART);
        request.target = name(target);
        request.target_identity = ident(&format!("container:x/{target}"));
        request.pinned = [request.target_identity.clone()].into();
        let d = eval(&rules, &request);
        assert_eq!(d.outcome, Allow, "{target}");
        assert_eq!(d.matched.len(), 1, "{target}: the sets are disjoint");
        let allowed = d.constraints.targets.expect("an Exact winner");
        assert!(allowed.contains(&name(target)), "{target}");
    }
}
