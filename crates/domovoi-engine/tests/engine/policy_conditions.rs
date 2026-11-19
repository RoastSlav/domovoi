//! Conditions, canonical arguments, input validation and the serde shapes.

use domovoi_core::capability::ConstraintKind;
use domovoi_engine::policy::{CanonicalArgs, Condition, EvaluateRequest, FactState, TimeOfDay};
use jiff::SignedDuration;

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

fn request_at(fx: &Fx, hour: i64, minute: i64) -> EvaluateRequest {
    let mut request = fx.req(RESTART);
    request.now = ts(hour, minute);
    request
}

#[test]
fn time_window_includes_start_excludes_end() {
    let fx = Fx::new();
    let day = window(at(9, 0), at(17, 0));
    for (hour, minute, inside) in [
        (8, 59, false),
        (9, 0, true),
        (9, 1, true),
        (16, 59, true),
        (17, 0, false),
        (23, 59, false),
    ] {
        assert_eq!(
            day.holds(&request_at(&fx, hour, minute)),
            inside,
            "{hour:02}:{minute:02}"
        );
    }
}

#[test]
fn time_window_wraps_across_midnight() {
    let fx = Fx::new();
    let night = window(at(22, 0), at(6, 0));
    for (hour, minute, inside) in [
        (21, 59, false),
        (22, 0, true),
        (23, 59, true),
        (0, 0, true),
        (5, 59, true),
        (6, 0, false),
        (12, 0, false),
    ] {
        assert_eq!(
            night.holds(&request_at(&fx, hour, minute)),
            inside,
            "{hour:02}:{minute:02}"
        );
    }
    let degenerate = window(at(0, 0), at(0, 0));
    assert!(!degenerate.holds(&request_at(&fx, 0, 0)));
}

#[test]
fn time_window_reads_utc_not_a_local_zone() {
    let fx = Fx::new();
    let mut request = fx.req(RESTART);
    request.now = jiff::Timestamp::from_second(86_400 * 400 + 3 * 3600).unwrap();
    assert!(window(at(3, 0), at(4, 0)).holds(&request));
    assert!(!window(at(4, 0), at(5, 0)).holds(&request));
}

#[test]
fn arg_constraint_needs_a_numeric_value() {
    let fx = Fx::new();
    let at_most = arg(ConstraintKind::IntAtMost, 60);
    let at_least = arg(ConstraintKind::IntAtLeast, 60);
    let rows = [
        ("wait_seconds:2:30\n", true, false),
        ("wait_seconds:2:60\n", true, true),
        ("wait_seconds:2:90\n", false, true),
        ("wait_seconds:2:-5\n", true, false),
        ("", false, false),
        ("other:2:30\n", false, false),
        ("wait_seconds:3:abc\n", false, false),
        ("wait_seconds:3:3.5\n", false, false),
        ("wait_seconds:0:\n", false, false),
        ("wait_seconds:2:+5\n", true, false),
        ("wait_seconds:20:99999999999999999999\n", false, false),
        ("wait_seconds:2:30\nbroken", true, false),
    ];
    for (args, most, least) in rows {
        let mut request = fx.req(RESTART);
        request.args = CanonicalArgs::from_canonical(args);
        assert_eq!(at_most.holds(&request), most, "at most / {args:?}");
        assert_eq!(at_least.holds(&request), least, "at least / {args:?}");
    }
}

#[test]
fn max_rate_and_max_ttl_match_within_bound() {
    let fx = Fx::new();
    let rate = Condition::MaxRate { max_per_hour: 5 };
    let ttl = Condition::MaxTtl {
        max_ttl: SignedDuration::from_secs(600),
    };
    for (asked, holds) in [
        (None, true),
        (Some(0), true),
        (Some(5), true),
        (Some(6), false),
    ] {
        let mut request = fx.req(RESTART);
        request.requested_rate_per_hour = asked;
        assert_eq!(rate.holds(&request), holds, "rate {asked:?}");
    }
    for (asked, holds) in [
        (None, true),
        (Some(1), true),
        (Some(600), true),
        (Some(601), false),
    ] {
        let mut request = fx.req(RESTART);
        request.requested_ttl = asked.map(SignedDuration::from_secs);
        assert_eq!(ttl.holds(&request), holds, "ttl {asked:?}");
    }
}

#[test]
fn requires_fact_matches_only_known_expected() {
    let fx = Fx::new();
    let states = [
        (None, false, false),
        (Some(FactState::Unknown), false, false),
        (Some(FactState::Stale), false, false),
        (Some(FactState::Disputed), false, false),
        (Some(FactState::Known(true)), true, false),
        (Some(FactState::Known(false)), false, true),
    ];
    for (state, wants_true, wants_false) in states {
        let mut request = fx.req(RESTART);
        if let Some(state) = state {
            request.facts.insert(fact(FACT), state);
        }
        for (expected, holds) in [(true, wants_true), (false, wants_false)] {
            let condition = Condition::RequiresFact {
                fact: fact(FACT),
                expected,
            };
            assert_eq!(
                condition.holds(&request),
                holds,
                "{state:?} expecting {expected}"
            );
        }
    }
}

#[test]
fn canonical_args_reads_name_length_value_lines() {
    let args = CanonicalArgs::from_canonical("a:1:x\nb:3:y\nz\nc:0:\nd:5:p:q:r\ndup:1:1\ndup:1:2");
    assert_eq!(args.get("a"), Some("x"));
    assert_eq!(
        args.get("b"),
        Some("y\nz"),
        "the length, not the newline, ends a value"
    );
    assert_eq!(args.get("c"), Some(""));
    assert_eq!(args.get("d"), Some("p:q:r"));
    assert_eq!(args.get("dup"), Some("1"), "the first line wins");
    assert_eq!(args.get("missing"), None);
    assert_eq!(CanonicalArgs::from_canonical("").get("a"), None);
}

#[test]
fn canonical_args_fail_closed_on_malformed_line() {
    let args = CanonicalArgs::from_canonical("a:1:x\nbad\nc:1:z\n");
    assert_eq!(args.get("a"), Some("x"));
    assert_eq!(args.get("bad"), None);
    assert_eq!(
        args.get("c"),
        None,
        "nothing after the first malformed line is read"
    );
    for malformed in [
        "a:9:x\n",
        "a:x:1\n",
        "a::1\n",
        ":1:x\n",
        "a:1:xy\n",
        "a:-1:x\n",
        "a:1\n",
        "a:99999999999999999999999:x\n",
        "a:1:é\n",
    ] {
        assert_eq!(
            CanonicalArgs::from_canonical(malformed).get("a"),
            None,
            "{malformed:?}"
        );
    }
}
