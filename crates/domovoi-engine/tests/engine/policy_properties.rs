//! Properties of the evaluator over generated rule sets.

use domovoi_core::capability::{ConstraintKind, EffectClass};
use domovoi_engine::policy::{
    CanonicalArgs, CapabilityScope, Condition, EvaluateRequest, Outcome, Outcome::Allow, Rule,
    RuleSelection, TargetScope, evaluate, evaluate_shadow, simulate,
};
use proptest::prelude::*;

use crate::policy_fixtures::*;

/// One generated rule, as plain data so the hosts can be chosen per test.
#[derive(Debug, Clone)]
struct Spec {
    host: u8,
    capability: u8,
    targets: u8,
    outcome: u8,
    locked: bool,
    shadow: bool,
    condition: u8,
}

fn spec(with_shadow: bool) -> impl Strategy<Value = Spec> {
    (
        0u8..3,
        0u8..5,
        0u8..4,
        0u8..3,
        any::<bool>(),
        any::<bool>(),
        0u8..4,
    )
        .prop_map(
            move |(host, capability, targets, outcome, locked, shadow, condition)| Spec {
                host,
                capability,
                targets,
                outcome,
                locked,
                shadow: with_shadow && shadow,
                condition,
            },
        )
}

fn build(fx: &Fx, specs: &[Spec]) -> Vec<Rule> {
    specs
        .iter()
        .enumerate()
        .map(|(n, s)| {
            let outcome =
                [Outcome::Allow, Outcome::RequireApproval, Outcome::Deny][s.outcome as usize];
            let mut r = rule(&format!("r{n}"), outcome);
            r.host = match s.host {
                0 => domovoi_engine::policy::HostScope::All,
                1 => domovoi_engine::policy::HostScope::Host(fx.host),
                _ => domovoi_engine::policy::HostScope::Host(fx.other),
            };
            r.capability = match s.capability {
                0 => CapabilityScope::Any,
                1 => CapabilityScope::Effect(EffectClass::Mutate),
                2 => CapabilityScope::Effect(EffectClass::Read),
                3 => CapabilityScope::Category(cat(LIFECYCLE)),
                _ => CapabilityScope::Contract(cid(RESTART)),
            };
            r.targets = match s.targets {
                0 => TargetScope::Any,
                1 => TargetScope::Exact(names(&["plex"])),
                2 => TargetScope::Exact(names(&["radarr"])),
                _ => TargetScope::Exact(names(&["plex", "radarr"])),
            };
            r.locked = s.locked && outcome == Outcome::Deny;
            r.shadow = s.shadow;
            r.conditions = match s.condition {
                1 => vec![Condition::MaxRate { max_per_hour: 5 }],
                2 => vec![Condition::TimeWindow {
                    start: domovoi_engine::policy::TimeOfDay { hour: 9, minute: 0 },
                    end: domovoi_engine::policy::TimeOfDay {
                        hour: 17,
                        minute: 0,
                    },
                }],
                3 => vec![Condition::ArgConstraint {
                    arg: "wait_seconds".to_owned(),
                    op: ConstraintKind::IntAtMost,
                    value: 60,
                }],
                _ => Vec::new(),
            };
            r
        })
        .collect()
}

#[derive(Debug, Clone)]
struct Ask {
    contract: u8,
    host: u8,
    target: u8,
    rate: Option<u32>,
    hour: i64,
    wait: u8,
}

fn ask() -> impl Strategy<Value = Ask> {
    (
        0u8..2,
        0u8..2,
        0u8..2,
        proptest::option::of(0u32..12),
        0i64..24,
        0u8..3,
    )
        .prop_map(|(contract, host, target, rate, hour, wait)| Ask {
            contract,
            host,
            target,
            rate,
            hour,
            wait,
        })
}

fn request(fx: &Fx, a: &Ask) -> EvaluateRequest {
    let mut r = fx.req([RESTART, READ][a.contract as usize]);
    r.host = if a.host == 0 { fx.host } else { fx.other };
    if a.target == 1 {
        r.target = name("radarr");
        r.target_identity = ident("container:def/radarr");
        r.pinned = [r.target_identity.clone()].into();
    }
    r.requested_rate_per_hour = a.rate;
    r.now = ts(a.hour, 0);
    r.args = match a.wait {
        0 => CanonicalArgs::from_canonical(""),
        1 => CanonicalArgs::from_canonical("wait_seconds:2:30\n"),
        _ => CanonicalArgs::from_canonical("wait_seconds:3:900\n"),
    };
    r
}

fn permute(rules: &[Rule], seed: u64) -> Vec<Rule> {
    let mut keyed: Vec<(u64, Rule)> = rules
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, r)| {
            let key = (seed ^ (i as u64 + 1))
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .rotate_left(17);
            (key, r)
        })
        .collect();
    keyed.sort_by_key(|(key, _)| *key);
    keyed.into_iter().map(|(_, r)| r).collect()
}

fn is_sorted<T: Ord>(items: &[T]) -> bool {
    items.windows(2).all(|w| w[0] <= w[1])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn evaluation_is_order_independent(
        specs in proptest::collection::vec(spec(false), 0..9),
        a in ask(),
        base in any::<u64>(),
    ) {
        let fx = Fx::new();
        let rules = build(&fx, &specs);
        let request = request(&fx, &a);
        let reference = evaluate(&rules, &request);
        let text = format!("{reference:?}");
        for n in 0..10u64 {
            let shuffled = permute(&rules, base.wrapping_add(n));
            let result = evaluate(&shuffled, &request);
            prop_assert_eq!(&result, &reference);
            prop_assert_eq!(format!("{result:?}"), text.clone());
        }
        let d = reference.unwrap();
        prop_assert!(is_sorted(&d.matched));
        prop_assert!(is_sorted(&d.winners));
        prop_assert!(is_sorted(&d.locked));
        prop_assert!(is_sorted(&d.overridden));
        prop_assert!(is_sorted(&d.warnings));
        prop_assert!(is_sorted(&d.facts_used));
    }

    #[test]
    fn simulator_matches_live_evaluator(
        specs in proptest::collection::vec(spec(true), 0..9),
        asks in proptest::collection::vec(ask(), 0..5),
    ) {
        let fx = Fx::new();
        let rules = build(&fx, &specs);
        let requests: Vec<EvaluateRequest> = asks.iter().map(|a| request(&fx, a)).collect();
        let enforced = simulate(&rules, &requests, RuleSelection::Enforced);
        let shadow = simulate(&rules, &requests, RuleSelection::Shadow);
        prop_assert_eq!(enforced.len(), requests.len());
        prop_assert_eq!(shadow.len(), requests.len());
        for (i, r) in requests.iter().enumerate() {
            prop_assert_eq!(&enforced[i], &evaluate(&rules, r));
            prop_assert_eq!(&shadow[i], &evaluate_shadow(&rules, r));
        }
    }

    #[test]
    fn allow_intersection_always_contains_the_requests_target(
        sets in proptest::collection::vec(proptest::option::of(1u8..16), 1..6),
        pick in 0usize..4,
    ) {
        let fx = Fx::new();
        let all = ["a", "b", "c", "d"];
        let rules: Vec<Rule> = sets
            .iter()
            .enumerate()
            .map(|(n, set)| {
                let mut r = rule(&format!("r{n}"), Allow).contract(RESTART);
                if let Some(bits) = set {
                    let chosen: Vec<&str> = (0..4).filter(|i| bits & (1 << i) != 0).map(|i| all[i]).collect();
                    r = r.named(&chosen);
                }
                r
            })
            .collect();
        let mut request = fx.req(RESTART);
        request.target = name(all[pick]);
        request.target_identity = ident(&format!("container:x/{}", all[pick]));
        request.pinned = [request.target_identity.clone()].into();
        let d = evaluate(&rules, &request).unwrap();
        if let Some(targets) = &d.constraints.targets {
            prop_assert!(d.outcome != Allow || targets.contains(&request.target), "{targets:?}");
        }
        if let Some(targets) = &d.constraints.targets {
            prop_assert!(!targets.is_empty());
        }
    }
}
