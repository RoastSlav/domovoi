//! The hard limits that cap an `Allow`: destructive effects, pinning, facts, the host
//! ceiling, root and autonomy.

use domovoi_core::capability::{CeilingEntry, CeilingTargets, EffectClass};
use domovoi_engine::policy::{
    CanonicalArgs, Condition, ConfirmationRequirement, EvaluateRequest, FactState, FactStatus,
    HardLimit, Outcome::Allow, Outcome::Deny, Outcome::RequireApproval, OutcomeReason, PolicyError,
    evaluate,
};
use jiff::SignedDuration;

use crate::policy_fixtures::*;

type Tweak = Box<dyn Fn(&mut EvaluateRequest)>;

fn tweak(f: impl Fn(&mut EvaluateRequest) + 'static) -> Tweak {
    Box::new(f)
}

fn entry(request: &mut EvaluateRequest) -> &mut CeilingEntry {
    let restart = cid(RESTART);
    request
        .ceiling
        .as_mut()
        .unwrap()
        .entries
        .iter_mut()
        .find(|e| e.contract == restart)
        .unwrap()
}

#[test]
fn destructive_never_allowed() {
    let fx = Fx::new();
    let d = eval(&[rule("allow", Allow)], &fx.req(DESTROY));
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(
        d.hard_limits_applied,
        vec![HardLimit::DestructiveNeverAllowed]
    );
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::Typed);
    assert_eq!(d.constraints, Default::default());
    let ordinary = d.would_have_been.expect("the limit lowered the outcome");
    assert_eq!(ordinary.outcome, Allow);
    assert_eq!(ordinary.reason, OutcomeReason::SingleWinner);
    assert!(ordinary.hard_limits_applied.is_empty());
}

#[test]
fn ceiling_caps_allow() {
    let fx = Fx::new();
    let rows: Vec<(&str, Tweak, bool)> = vec![
        ("covering ceiling", tweak(|_| {}), true),
        (
            "named target",
            tweak(|r| entry(r).targets = CeilingTargets::Names(names(&["plex"]))),
            true,
        ),
        (
            "rate at the maximum",
            tweak(|r| r.requested_rate_per_hour = Some(10)),
            true,
        ),
        (
            "ttl within the bound",
            tweak(|r| {
                entry(r).max_ttl = Some(SignedDuration::from_secs(600));
                r.requested_ttl = Some(SignedDuration::from_secs(600));
            }),
            true,
        ),
        (
            "argument within the bound",
            tweak(|r| {
                entry(r).args.insert("wait_seconds".to_owned(), 60);
                r.args = CanonicalArgs::from_canonical("wait_seconds:2:30\n");
            }),
            true,
        ),
        (
            "entry absent",
            tweak(|r| r.ceiling.as_mut().unwrap().entries.clear()),
            false,
        ),
        (
            "entry for another contract",
            tweak(|r| {
                r.ceiling
                    .as_mut()
                    .unwrap()
                    .entries
                    .retain(|e| e.contract != cid(RESTART))
            }),
            false,
        ),
        (
            "ceiling disabled",
            tweak(|r| r.ceiling.as_mut().unwrap().enabled = false),
            false,
        ),
        (
            "target not in Names",
            tweak(|r| entry(r).targets = CeilingTargets::Names(names(&["radarr"]))),
            false,
        ),
        (
            "rate above the maximum",
            tweak(|r| r.requested_rate_per_hour = Some(11)),
            false,
        ),
        (
            "ttl above the bound",
            tweak(|r| {
                entry(r).max_ttl = Some(SignedDuration::from_secs(600));
                r.requested_ttl = Some(SignedDuration::from_secs(900));
            }),
            false,
        ),
        (
            "argument above the bound",
            tweak(|r| {
                entry(r).args.insert("wait_seconds".to_owned(), 60);
                r.args = CanonicalArgs::from_canonical("wait_seconds:2:90\n");
            }),
            false,
        ),
        (
            "bounded argument missing",
            tweak(|r| {
                entry(r).args.insert("wait_seconds".to_owned(), 60);
            }),
            false,
        ),
        (
            "bounded argument not a number",
            tweak(|r| {
                entry(r).args.insert("wait_seconds".to_owned(), 60);
                r.args = CanonicalArgs::from_canonical("wait_seconds:3:abc\n");
            }),
            false,
        ),
    ];
    for (label, change, covered) in &rows {
        let mut request = fx.req(RESTART);
        change(&mut request);
        let d = eval(&[rule("allow", Allow)], &request);
        if *covered {
            assert_eq!(d.outcome, Allow, "{label}");
            assert!(d.hard_limits_applied.is_empty(), "{label}");
        } else {
            assert_eq!(d.outcome, RequireApproval, "{label}");
            assert_eq!(
                d.hard_limits_applied,
                vec![HardLimit::CeilingDoesNotCover],
                "{label}"
            );
            assert_eq!(
                d.would_have_been.as_ref().map(|w| w.outcome),
                Some(Allow),
                "{label}"
            );
        }
    }
}

#[test]
fn missing_ceiling_is_not_allowed() {
    let fx = Fx::new();
    for contract in [RESTART, READ] {
        let mut request = fx.req(contract);
        request.ceiling = None;
        let d = eval(&[rule("allow", Allow)], &request);
        assert_eq!(d.outcome, RequireApproval, "{contract}");
        assert_eq!(
            d.hard_limits_applied,
            vec![HardLimit::CeilingDoesNotCover],
            "{contract}"
        );
        assert_eq!(d.ceiling_hash, None, "{contract}");
    }
}

#[test]
fn stale_ceiling_fails_coverage() {
    let fx = Fx::new();
    let rules = [rule("allow", Allow)];
    let hash = |request: &mut EvaluateRequest, expected: Option<&str>, summary: Option<&str>| {
        request.expected_ceiling_hash = expected.map(str::to_owned);
        request.ceiling.as_mut().unwrap().hash = summary.map(str::to_owned);
    };
    let rows = [
        (Some("h1"), Some("h1"), true),
        (None, Some("anything"), true),
        (Some("old"), Some("h1"), false),
        (Some("h1"), None, false),
    ];
    for (expected, summary, covered) in rows {
        let mut request = fx.req(RESTART);
        hash(&mut request, expected, summary);
        let d = eval(&rules, &request);
        let row = format!("expected {expected:?}, reported {summary:?}");
        assert_eq!(d.outcome == Allow, covered, "{row}");
        if !covered {
            assert_eq!(
                d.hard_limits_applied,
                vec![HardLimit::CeilingDoesNotCover],
                "{row}"
            );
        }
        assert_eq!(
            d.ceiling_hash.as_deref(),
            summary,
            "{row}: the hash is echoed"
        );
    }
    let mut request = fx.req(RESTART);
    request.ceiling = None;
    request.expected_ceiling_hash = Some("h1".to_owned());
    assert_eq!(
        evaluate(&rules, &request),
        Err(PolicyError::CeilingHashMissing)
    );
}

#[test]
fn unknown_safety_fact_never_allows() {
    let fx = Fx::new();
    let states = [
        (None, false),
        (Some(FactState::Unknown), false),
        (Some(FactState::Stale), false),
        (Some(FactState::Disputed), false),
        (Some(FactState::Known(false)), false),
        (Some(FactState::Known(true)), true),
    ];
    let needs_fact = rule("needs_fact", Allow)
        .contract(RESTART)
        .when(Condition::RequiresFact {
            fact: fact(FACT),
            expected: true,
        });
    for (state, matches) in states {
        let mut request = fx.req(RESTART);
        if let Some(state) = state {
            request.facts.insert(fact(FACT), state);
        }
        let d = eval(std::slice::from_ref(&needs_fact), &request);
        assert_eq!(d.matched.len() == 1, matches, "{state:?}");
        assert_eq!(d.outcome == Allow, matches, "{state:?}");
        if !matches {
            assert_eq!(d.reason, OutcomeReason::Default, "{state:?}");
        }
    }

    let declared = [
        (None, FactStatus::Unknown, false),
        (Some(FactState::Unknown), FactStatus::Unknown, false),
        (Some(FactState::Stale), FactStatus::Stale, false),
        (Some(FactState::Disputed), FactStatus::Disputed, false),
        (Some(FactState::Known(true)), FactStatus::Known, true),
    ];
    for (state, status, allowed) in declared {
        let mut request = fx.req(GATED);
        if let Some(state) = state {
            request.facts.insert(fact(FACT), state);
        }
        let d = eval(&[rule("allow", Allow)], &request);
        assert_eq!(d.facts_used, vec![(fact(FACT), status)], "{state:?}");
        if allowed {
            assert_eq!(d.outcome, Allow, "{state:?}");
        } else {
            assert_eq!(d.outcome, RequireApproval, "{state:?}");
            assert_eq!(
                d.hard_limits_applied,
                vec![HardLimit::SafetyFactUnknown],
                "{state:?}"
            );
            assert_eq!(
                d.typed_confirmation,
                ConfirmationRequirement::None,
                "{state:?}"
            );
        }
    }
}

#[test]
fn model_target_cannot_widen_scope() {
    let fx = Fx::new();
    let plex = ident("container:abc/plex");
    let other = ident("container:zzz/other");
    let rows: Vec<(&str, Tweak, bool, bool)> = vec![
        (
            "not pinned, not resolved",
            tweak(|r| r.pinned.clear()),
            true,
            false,
        ),
        (
            "named by the rule and resolved to the target",
            tweak(move |r| {
                r.pinned.clear();
                r.rule_targets.insert(name("plex"), plex.clone());
            }),
            true,
            true,
        ),
        (
            "name resolves to another identity",
            tweak(move |r| {
                r.pinned.clear();
                r.rule_targets.insert(name("plex"), other.clone());
            }),
            true,
            false,
        ),
        (
            "resolution for a name the rule does not carry",
            tweak(|r| {
                r.pinned.clear();
                r.rule_targets
                    .insert(name("radarr"), ident("container:abc/plex"));
            }),
            true,
            false,
        ),
        ("pinned", tweak(|_| {}), true, true),
        (
            "an Any rule cannot widen the pinned set",
            tweak(|r| {
                r.pinned.clear();
                r.rule_targets
                    .insert(name("plex"), ident("container:abc/plex"));
            }),
            false,
            false,
        ),
    ];
    for (label, change, named, allowed) in &rows {
        let mut request = fx.req(RESTART);
        change(&mut request);
        let allow = if *named {
            rule("allow", Allow).named(&["plex"])
        } else {
            rule("allow", Allow)
        };
        let d = eval(&[allow], &request);
        if *allowed {
            assert_eq!(d.outcome, Allow, "{label}");
        } else {
            assert_eq!(d.outcome, RequireApproval, "{label}");
            assert_eq!(
                d.hard_limits_applied,
                vec![HardLimit::TargetNotPinned],
                "{label}"
            );
        }
    }
}

#[test]
fn deny_is_not_offered_for_approval() {
    let fx = Fx::new();
    let mut overlap = fx.req(CONTAIN);
    overlap.containment.target_overlaps_verified_set = true;
    let cases = [
        ("plain deny", vec![rule("d", Deny)], fx.req(DESTROY)),
        (
            "locked deny",
            vec![rule("d", Deny).locked()],
            fx.req(DESTROY),
        ),
        ("limit-made deny", vec![rule("a", Allow)], overlap),
    ];
    for (label, rules, request) in cases {
        let d = eval(&rules, &request);
        assert_eq!(d.outcome, Deny, "{label}");
        assert_ne!(d.outcome, RequireApproval, "{label}");
        assert_eq!(
            d.typed_confirmation,
            ConfirmationRequirement::None,
            "{label}"
        );
        assert_eq!(d.constraints, Default::default(), "{label}");
    }
}

#[test]
fn root_agent_is_never_allowed() {
    let fx = Fx::new();
    let mut request = fx.req(RESTART);
    request.runs_as_root = true;
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(d.hard_limits_applied, vec![HardLimit::AgentRunsAsRoot]);

    let mut read = fx.req(READ);
    read.runs_as_root = true;
    let d = eval(&[], &read);
    assert_eq!(
        d.outcome, RequireApproval,
        "even the read default is not unattended as root"
    );
    assert_eq!(d.hard_limits_applied, vec![HardLimit::AgentRunsAsRoot]);

    let d = eval(&[], &request);
    assert_eq!(d.outcome, RequireApproval);
    assert!(
        d.hard_limits_applied.is_empty(),
        "nothing to cap when the ordinary result is not Allow"
    );
}

#[test]
fn paused_autonomy_requires_approval() {
    let fx = Fx::new();
    let mut request = fx.req(RESTART);
    request.autonomy.paused = true;
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(d.hard_limits_applied, vec![HardLimit::AutonomyInactive]);
    assert_eq!(d.would_have_been.as_ref().map(|w| w.outcome), Some(Allow));

    let d = eval(&[], &request);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(d.hard_limits_applied, vec![HardLimit::AutonomyInactive]);
    assert!(d.would_have_been.is_none());

    let mut read = fx.req(READ);
    read.autonomy.paused = true;
    assert_eq!(eval(&[], &read).outcome, RequireApproval);

    let d = eval(&[rule("deny", Deny)], &request);
    assert_eq!(d.outcome, Deny, "a pause never softens a deny");
}

#[test]
fn circuit_broken_rule_requires_approval() {
    let fx = Fx::new();
    let mut request = fx.req(RESTART);
    request.autonomy.circuit_broken = true;
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(d.hard_limits_applied, vec![HardLimit::AutonomyInactive]);
    assert_eq!(eval(&[rule("deny", Deny)], &request).outcome, Deny);
}

#[test]
fn several_hard_limits_apply_in_trace_order() {
    let fx = Fx::new();
    let mut request = fx.req(PURGE);
    request.ceiling = None;
    request.autonomy.paused = true;
    request.runs_as_root = true;
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(
        d.hard_limits_applied,
        vec![
            HardLimit::DestructiveNeverAllowed,
            HardLimit::SafetyFactUnknown,
            HardLimit::CeilingDoesNotCover,
            HardLimit::AutonomyInactive,
            HardLimit::AgentRunsAsRoot,
        ]
    );
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::Typed);
    assert_eq!(d.would_have_been.as_ref().map(|w| w.outcome), Some(Allow));
    let mut sorted = d.hard_limits_applied.clone();
    sorted.sort();
    assert_eq!(
        sorted, d.hard_limits_applied,
        "declaration order is trace order"
    );
    assert_eq!(
        fx.index.meta(&cid(PURGE)).unwrap().effect,
        EffectClass::Destructive
    );
}
