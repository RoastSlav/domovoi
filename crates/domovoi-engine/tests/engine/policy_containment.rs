//! Containment readiness and how the hard limits compose: they only ever raise severity.

use domovoi_engine::policy::{
    ConfirmationRequirement, EvaluateRequest, HardLimit, ManagementPath, Outcome, Outcome::Allow,
    Outcome::Deny, Outcome::RequireApproval, OutcomeReason, UnavailableReason, Warning,
};

use crate::policy_fixtures::*;

type Trigger = fn(&mut EvaluateRequest);

const UNAVAILABLE: Outcome = Outcome::Unavailable(UnavailableReason::SafetyUnknown);

fn path(text: &str) -> ManagementPath {
    ManagementPath::parse(text).unwrap()
}

fn contain(fx: &Fx) -> EvaluateRequest {
    fx.req(CONTAIN)
}

#[test]
fn containment_exclusion_denies_even_with_allow() {
    let fx = Fx::new();
    let mut request = contain(&fx);
    request.containment.target_overlaps_verified_set = true;
    for (label, rules, ordinary) in [
        ("allow", vec![rule("r", Allow)], Some(Allow)),
        (
            "approval",
            vec![rule("r", RequireApproval)],
            Some(RequireApproval),
        ),
        ("no rule", vec![], Some(RequireApproval)),
        ("deny", vec![rule("r", Deny)], None),
    ] {
        let d = eval(&rules, &request);
        assert_eq!(d.outcome, Deny, "{label}");
        assert_eq!(
            d.hard_limits_applied,
            vec![HardLimit::ContainmentTargetExcluded],
            "{label}"
        );
        assert_eq!(
            d.typed_confirmation,
            ConfirmationRequirement::None,
            "{label}"
        );
        assert_eq!(
            d.would_have_been.as_ref().map(|w| w.outcome),
            ordinary,
            "{label}"
        );
    }
}

#[test]
fn containment_requires_confirmed_management_paths() {
    let fx = Fx::new();
    let docker = path("/var/run/docker.sock");
    let podman = path("/run/podman/podman.sock");

    let mut uncovered = contain(&fx);
    uncovered.containment.uncovered_paths = vec![podman.clone(), docker.clone(), podman.clone()];
    let d = eval(&[rule("allow", Allow)], &uncovered);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(
        d.hard_limits_applied,
        vec![HardLimit::ManagementPathUncovered]
    );
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::Typed);
    assert_eq!(
        d.warnings,
        vec![Warning::ManagementPathsUnprotected {
            paths: vec![podman.clone(), docker.clone()]
        }],
        "paths are sorted and deduplicated"
    );
    assert!(podman < docker, "the expected order is by path text");
    assert_eq!(d.would_have_been.as_ref().map(|w| w.outcome), Some(Allow));

    let mut stale = contain(&fx);
    stale.containment.stale_confirmation = true;
    let d = eval(&[rule("allow", Allow)], &stale);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(
        d.hard_limits_applied,
        vec![HardLimit::ManagementPathUncovered]
    );
    assert_eq!(
        d.warnings,
        vec![Warning::ManagementPathsUnprotected { paths: vec![] }]
    );

    let d = eval(&[], &uncovered);
    assert_eq!(d.outcome, RequireApproval);
    assert_eq!(
        d.hard_limits_applied,
        vec![HardLimit::ManagementPathUncovered]
    );
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::Typed);
    assert!(
        d.would_have_been.is_none(),
        "the default already required approval"
    );

    assert_eq!(eval(&[rule("allow", Allow)], &contain(&fx)).outcome, Allow);
}

#[test]
fn missing_containment_attestation_requires_typed_confirmation() {
    let fx = Fx::new();
    let mut request = contain(&fx);
    request.containment.attested = false;
    for rules in [vec![rule("allow", Allow)], vec![]] {
        let d = eval(&rules, &request);
        assert_eq!(d.outcome, RequireApproval);
        assert_eq!(
            d.hard_limits_applied,
            vec![HardLimit::ContainmentUnverified]
        );
        assert_eq!(d.typed_confirmation, ConfirmationRequirement::Typed);
        assert_eq!(d.warnings, vec![Warning::SafetySetUnverified]);
    }
    request.containment.uncovered_paths = vec![path("/var/run/docker.sock")];
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(
        d.hard_limits_applied,
        vec![
            HardLimit::ContainmentUnverified,
            HardLimit::ManagementPathUncovered
        ]
    );
    assert_eq!(d.warnings.len(), 2);
    assert_eq!(d.warnings[0], Warning::SafetySetUnverified);
}

#[test]
fn incomplete_containment_readiness_is_unavailable() {
    let fx = Fx::new();
    let mut request = contain(&fx);
    request.containment.helper_incomplete = true;
    for rules in [vec![rule("allow", Allow)], vec![]] {
        let d = eval(&rules, &request);
        assert_eq!(d.outcome, UNAVAILABLE);
        assert_eq!(
            d.hard_limits_applied,
            vec![HardLimit::HelperSafetyIncomplete]
        );
        assert_eq!(d.typed_confirmation, ConfirmationRequirement::None);
        assert!(d.warnings.is_empty());
    }
    request.containment.target_overlaps_verified_set = true;
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(
        d.outcome, Deny,
        "an exclusion outranks an unavailable result"
    );
    assert_eq!(
        d.hard_limits_applied,
        vec![
            HardLimit::ContainmentTargetExcluded,
            HardLimit::HelperSafetyIncomplete
        ]
    );
}

#[test]
fn deny_survives_helper_incomplete_and_autonomy_limits() {
    let fx = Fx::new();
    let mut request = contain(&fx);
    request.containment.helper_incomplete = true;
    request.autonomy.paused = true;
    request.autonomy.circuit_broken = true;
    let d = eval(&[rule("deny", Deny)], &request);
    assert_eq!(d.outcome, Deny);
    assert_eq!(
        d.hard_limits_applied,
        vec![
            HardLimit::HelperSafetyIncomplete,
            HardLimit::AutonomyInactive
        ]
    );
    assert!(d.would_have_been.is_none(), "the outcome did not change");
}

#[test]
fn unavailable_survives_later_require_approval_limits() {
    let fx = Fx::new();
    let mut request = contain(&fx);
    request.containment.helper_incomplete = true;
    request.autonomy.paused = true;
    request.runs_as_root = true;
    let d = eval(&[rule("allow", Allow)], &request);
    assert_eq!(d.outcome, UNAVAILABLE);
    assert_eq!(
        d.hard_limits_applied,
        vec![
            HardLimit::HelperSafetyIncomplete,
            HardLimit::AutonomyInactive,
            HardLimit::AgentRunsAsRoot,
        ]
    );
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::None);
    assert_eq!(d.would_have_been.as_ref().map(|w| w.outcome), Some(Allow));
}

#[test]
fn typed_confirmation_survives_a_more_severe_start() {
    let fx = Fx::new();
    let mut paused = fx.req(DESTROY);
    paused.autonomy.paused = true;
    paused.runs_as_root = true;
    for (label, rules, limits) in [
        ("default", vec![], vec![HardLimit::AutonomyInactive]),
        (
            "approval rule",
            vec![rule("r", RequireApproval)],
            vec![HardLimit::AutonomyInactive],
        ),
        (
            "allow rule",
            vec![rule("r", Allow)],
            vec![
                HardLimit::DestructiveNeverAllowed,
                HardLimit::AutonomyInactive,
                HardLimit::AgentRunsAsRoot,
            ],
        ),
    ] {
        let d = eval(&rules, &paused);
        assert_eq!(d.outcome, RequireApproval, "{label}");
        assert_eq!(
            d.typed_confirmation,
            ConfirmationRequirement::Typed,
            "{label}"
        );
        assert_eq!(d.hard_limits_applied, limits, "{label}");
    }
    let d = eval(&[rule("r", Deny)], &paused);
    assert_eq!(d.typed_confirmation, ConfirmationRequirement::None);
}

#[test]
fn locked_deny_survives_every_hard_limit() {
    let fx = Fx::new();
    let mut request = contain(&fx);
    request.ceiling = None;
    request.runs_as_root = true;
    request.autonomy.paused = true;
    request.autonomy.circuit_broken = true;
    request.pinned.clear();
    request.containment.attested = false;
    request.containment.stale_confirmation = true;
    request.containment.helper_incomplete = true;
    request.containment.target_overlaps_verified_set = true;
    request.containment.uncovered_paths = vec![path("/var/run/docker.sock")];
    let rules = [
        rule("lock", Deny).locked(),
        rule("allow", Allow).contract(CONTAIN),
    ];
    let d = eval(&rules, &request);
    assert_eq!(d.outcome, Deny);
    assert_eq!(d.reason, OutcomeReason::LockedDeny);
    assert!(d.hard_limits_applied.is_empty());
    assert!(d.warnings.is_empty());
    assert!(d.would_have_been.is_some());
}

#[test]
fn hard_limits_never_lower_severity() {
    let fx = Fx::new();
    let triggers: Vec<(&str, Trigger)> = vec![
        ("none", |_| {}),
        ("no ceiling", |r| r.ceiling = None),
        ("unpinned", |r| r.pinned.clear()),
        ("paused", |r| r.autonomy.paused = true),
        ("circuit broken", |r| r.autonomy.circuit_broken = true),
        ("root", |r| r.runs_as_root = true),
        ("unattested", |r| r.containment.attested = false),
        ("stale confirmation", |r| {
            r.containment.stale_confirmation = true
        }),
        ("overlap", |r| {
            r.containment.target_overlaps_verified_set = true
        }),
        ("helper incomplete", |r| {
            r.containment.helper_incomplete = true
        }),
        ("everything", |r| {
            r.ceiling = None;
            r.pinned.clear();
            r.runs_as_root = true;
            r.autonomy.paused = true;
            r.containment.attested = false;
            r.containment.helper_incomplete = true;
            r.containment.target_overlaps_verified_set = true;
        }),
    ];
    let starts = [
        ("allow", vec![rule("r", Allow)]),
        ("approval", vec![rule("r", RequireApproval)]),
        ("deny", vec![rule("r", Deny)]),
        ("default", vec![]),
    ];
    for (start, rules) in &starts {
        for (trigger, change) in &triggers {
            let mut request = contain(&fx);
            change(&mut request);
            let d = eval(rules, &request);
            let ordinary = d
                .would_have_been
                .as_deref()
                .map_or(d.outcome, |w| w.outcome);
            let row = format!("{start} / {trigger}");
            assert!(
                d.outcome >= ordinary,
                "{row}: {:?} < {ordinary:?}",
                d.outcome
            );
            if d.outcome == Allow {
                assert_eq!(ordinary, Allow, "{row}");
                assert!(d.hard_limits_applied.is_empty(), "{row}");
            }
            if ordinary == Deny {
                assert_eq!(d.outcome, Deny, "{row}");
            }
        }
    }
}
