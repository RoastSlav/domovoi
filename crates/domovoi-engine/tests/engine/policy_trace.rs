//! The trace of a decision: which rules overrode which, and the warnings that follow.

use domovoi_core::capability::EffectClass;
use domovoi_engine::policy::{
    Condition, Outcome::Allow, Outcome::Deny, Outcome::RequireApproval, OutcomeReason, Overridden,
    Warning,
};

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
fn overridden_trace_includes_same_outcome_broader_rule() {
    let fx = Fx::new();
    let rules = [
        rule("broad", Allow).category(LIFECYCLE),
        rule("narrow", Allow).contract(RESTART),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Allow);
    assert_eq!(d.overridden, vec![over("broad", "narrow")]);
    assert!(
        d.warnings.is_empty(),
        "only an Allow over a Deny is a dangerous exception"
    );
}

#[test]
fn specific_allow_over_broad_deny_emits_dangerous_exception() {
    let fx = Fx::new();
    let rules = [
        rule("deny_cat", Deny).category(LIFECYCLE),
        rule("deny_eff", Deny).effect(EffectClass::Mutate),
        rule("approve", RequireApproval)
            .category(LIFECYCLE)
            .on_host(fx.host),
        rule("allow", Allow)
            .contract(RESTART)
            .named(&["plex"])
            .on_host(fx.host),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert_eq!(d.outcome, Allow);
    assert_eq!(
        d.warnings,
        vec![danger("allow", "deny_cat"), danger("allow", "deny_eff")],
        "one warning per overridden deny, none for the approval rule"
    );
    assert_eq!(
        d.overridden,
        vec![
            over("approve", "allow"),
            over("deny_cat", "allow"),
            over("deny_eff", "allow"),
        ]
    );
}

#[test]
fn equal_scope_rules_are_not_overrides() {
    let fx = Fx::new();
    let rules = [
        rule("a", Allow).contract(RESTART).named(&["plex"]),
        rule("b", Allow).contract(RESTART).named(&["plex"]),
        rule("c", Deny).contract(RESTART).named(&["plex"]),
    ];
    let d = eval(&rules, &fx.req(RESTART));
    assert!(d.overridden.is_empty());
    assert!(d.warnings.is_empty());
    assert_eq!(d.winners, ids(&["a", "b", "c"]));
}

#[test]
fn overridden_trace_is_order_independent() {
    let fx = Fx::new();
    let rules = vec![
        rule("z_broad", Deny),
        rule("m_mid", RequireApproval).category(LIFECYCLE),
        rule("a_narrow", Allow).contract(RESTART).named(&["plex"]),
        rule("b_narrow", Allow).on_host(fx.host).contract(RESTART),
    ];
    let reference = eval(&rules, &fx.req(RESTART));
    assert_eq!(
        reference.overridden,
        vec![
            over("m_mid", "a_narrow"),
            over("m_mid", "b_narrow"),
            over("z_broad", "a_narrow"),
            over("z_broad", "b_narrow"),
        ]
    );
    let mut reversed = rules.clone();
    reversed.reverse();
    let mut rotated = rules.clone();
    rotated.rotate_left(2);
    for other in [reversed, rotated] {
        let d = eval(&other, &fx.req(RESTART));
        assert_eq!(d.overridden, reference.overridden);
        assert_eq!(d.warnings, reference.warnings);
        assert_eq!(d, reference);
    }
}

#[test]
fn locked_deny_records_would_have_been() {
    let fx = Fx::new();
    let lock = rule("lock", Deny).locked().named(&["plex"]);
    for (label, rules, ordinary, reason) in [
        (
            "alone",
            vec![lock.clone()],
            Deny,
            OutcomeReason::SingleWinner,
        ),
        (
            "with allow",
            vec![lock.clone(), rule("allow", Allow).named(&["plex"])],
            Deny,
            OutcomeReason::EqualScopeDeny,
        ),
    ] {
        let d = eval(&rules, &fx.req(RESTART));
        assert_eq!(d.outcome, Deny, "{label}");
        let inner = d.would_have_been.as_deref().unwrap_or_else(|| {
            panic!("{label}: a locked deny always carries the ordinary decision")
        });
        assert_eq!(inner.outcome, ordinary, "{label}");
        assert_eq!(inner.reason, reason, "{label}");
        assert!(inner.would_have_been.is_none(), "{label}");
        assert!(inner.locked.is_empty(), "{label}");
    }
    let d = eval(
        &[
            rule("lock", Deny).locked(),
            rule("allow", Allow).contract(RESTART),
        ],
        &fx.req(RESTART),
    );
    let inner = d.would_have_been.unwrap();
    assert_eq!(
        (inner.outcome, inner.reason),
        (Allow, OutcomeReason::SingleWinner)
    );
    assert_eq!(inner.winners, ids(&["allow"]));
}

#[test]
fn strict_specificity_ignores_conditions() {
    let fx = Fx::new();
    let request = fx.req(RESTART);
    let with_cond = Condition::MaxRate { max_per_hour: 5 };
    let rules = [
        rule("broad", Deny).category(LIFECYCLE),
        rule("narrow", Allow)
            .contract(RESTART)
            .when(with_cond.clone())
            .when(Condition::MaxRate { max_per_hour: 6 }),
    ];
    let d = eval(&rules, &request);
    assert_eq!(
        d.winners,
        ids(&["narrow"]),
        "conditions do not widen or narrow scope"
    );
    assert_eq!(d.outcome, Allow);

    let same_scope = [
        rule("plain", Allow).contract(RESTART),
        rule("conditioned", Deny).contract(RESTART).when(with_cond),
    ];
    let d = eval(&same_scope, &request);
    assert_eq!(
        (d.outcome, d.reason),
        (Deny, OutcomeReason::EqualScopeDeny),
        "a condition does not make a rule more specific"
    );
    assert!(d.overridden.is_empty());
}
