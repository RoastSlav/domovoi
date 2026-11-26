//! The self-test's verdicts, its stored result and the drills' file.

use domovoi_core::containment::{DrillsState, Gate, UntestedReason, Verdict, gate_for};
use domovoi_helper::nft::drill::{DrillsFile, read, state_of, write};
use domovoi_helper::nft::selftest::verdict::{
    Counts, MIN_REPLACEMENTS, lowest_replacements, verdict,
};
use domovoi_helper::nft::selftest::{
    current_key, gate_text, read_result, up_to_date, write_result,
};
use jiff::Timestamp;

use super::world::World;

fn counts(leaked: u64, replacements: u64, control_leaked: u64) -> Counts {
    Counts {
        name: "host_lone".to_owned(),
        leaked,
        sent: 1_000_000,
        replacements,
        control_leaked,
        control_sent: 1_000_000,
        fault: None,
    }
}

fn result(hosts: Verdict, prefixes: Option<Verdict>) -> domovoi_core::containment::SelftestResult {
    domovoi_core::containment::SelftestResult {
        key: current_key("6.8.0", "nftables v1.0.9"),
        hosts,
        prefixes,
        ran_at: "2026-01-01T00:00:00Z"
            .parse::<Timestamp>()
            .expect("timestamp"),
        replacements: MIN_REPLACEMENTS,
        control_leak_ratio: 0.4,
    }
}

#[test]
fn pass_needs_no_leak_a_leaking_control_and_the_replacement_minimum() {
    assert_eq!(
        verdict(&[counts(0, MIN_REPLACEMENTS, 400_000)]),
        Verdict::Pass
    );
}

#[test]
fn any_leaked_packet_fails_the_set_type() {
    assert_eq!(
        verdict(&[
            counts(0, MIN_REPLACEMENTS, 400_000),
            counts(3, MIN_REPLACEMENTS, 400_000)
        ]),
        Verdict::Fail { leaked: 3 }
    );
}

#[test]
fn leak_self_test_replacements_not_reached_is_inconclusive() {
    let v = verdict(&[counts(0, MIN_REPLACEMENTS - 1, 400_000)]);
    assert_eq!(
        v,
        Verdict::Inconclusive {
            reason: "replacements_not_reached".to_owned()
        }
    );
    assert_eq!(lowest_replacements(&[counts(0, 5, 1), counts(0, 3, 1)]), 3);
}

#[test]
fn a_control_that_does_not_leak_cannot_prove_the_test_sees_a_window() {
    let v = verdict(&[counts(0, MIN_REPLACEMENTS, 0)]);
    assert_eq!(
        v,
        Verdict::Inconclusive {
            reason: "control_did_not_leak".to_owned()
        }
    );
}

#[test]
fn failure_to_run_the_self_test_is_not_a_pass() {
    assert!(matches!(verdict(&[]), Verdict::Inconclusive { .. }));
    let mut c = counts(0, MIN_REPLACEMENTS, 400_000);
    c.fault = Some("the namespace could not be created".to_owned());
    assert!(matches!(verdict(&[c]), Verdict::Inconclusive { .. }));
}

#[test]
fn leak_self_test_inconclusive_blocks_containment() {
    let key = current_key("6.8.0", "nftables v1.0.9");
    let stored = result(
        Verdict::Inconclusive {
            reason: "x".to_owned(),
        },
        None,
    );
    assert_eq!(
        gate_for(Some(&stored), &key, true),
        Gate::BlockedUntested {
            reason: UntestedReason::Inconclusive
        }
    );
}

#[test]
fn selftest_result_is_keyed_by_kernel_nft_and_layout() {
    let stored = result(Verdict::Pass, Some(Verdict::Pass));
    let now = stored.ran_at;
    assert!(up_to_date(Some(&stored), &stored.key, now));
    for other in [
        current_key("6.9.0", "nftables v1.0.9"),
        current_key("6.8.0", "nftables v1.1.0"),
    ] {
        assert!(!up_to_date(Some(&stored), &other, now));
        assert_eq!(
            gate_for(Some(&stored), &other, true),
            Gate::BlockedUntested {
                reason: UntestedReason::KeyChanged
            }
        );
    }
    assert!(!up_to_date(None, &stored.key, now));
}

#[test]
fn gate_mapping_matches_the_table_of_6_6() {
    let key = current_key("6.8.0", "nftables v1.0.9");
    let both = result(Verdict::Pass, Some(Verdict::Pass));
    let hosts_only = result(Verdict::Pass, Some(Verdict::Fail { leaked: 1 }));
    let leaking = result(Verdict::Fail { leaked: 2 }, Some(Verdict::Pass));
    assert_eq!(
        gate_for(Some(&both), &key, true),
        Gate::Open {
            interval_path: true
        }
    );
    assert_eq!(
        gate_for(Some(&hosts_only), &key, true),
        Gate::Open {
            interval_path: false
        }
    );
    assert_eq!(gate_for(Some(&leaking), &key, true), Gate::BlockedLeak);
    assert_eq!(
        gate_for(None, &key, true),
        Gate::BlockedUntested {
            reason: UntestedReason::NeverRun
        }
    );
    assert_eq!(
        gate_for(Some(&both), &key, false),
        Gate::BlockedUntested {
            reason: UntestedReason::PlatformUntested
        }
    );
}

#[test]
fn a_stale_inconclusive_result_is_run_again_after_seven_days() {
    let stored = result(
        Verdict::Pass,
        Some(Verdict::Inconclusive {
            reason: "x".to_owned(),
        }),
    );
    let later = stored.ran_at + jiff::SignedDuration::from_hours(8 * 24);
    assert!(up_to_date(Some(&stored), &stored.key, stored.ran_at));
    assert!(!up_to_date(Some(&stored), &stored.key, later));
}

#[test]
fn the_result_and_drill_files_round_trip_under_the_lock_with_mode_0600() {
    use std::os::unix::fs::PermissionsExt;
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let stored = result(Verdict::Pass, Some(Verdict::Pass));
    write_result(&guard, w.path(), &stored).expect("write");
    assert_eq!(read_result(w.path()).expect("read"), stored);
    let mode = std::fs::metadata(w.path().join("nft-selftest.json"))
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let file = DrillsFile {
        key: stored.key.clone(),
        state: DrillsState::Passed { at: stored.ran_at },
        results: std::iter::once(("drill_excluded_address_refused".to_owned(), "ok".to_owned()))
            .collect(),
    };
    write(&guard, w.path(), &file).expect("write drills");
    assert_eq!(read(w.path()), Some(file.clone()));
    assert_eq!(state_of(w.path(), &stored.key), file.state);
    assert_eq!(
        state_of(w.path(), &current_key("other", "other")),
        DrillsState::NotRun,
        "a result for another key reads as not run"
    );
    assert!(gate_text(&Gate::BlockedLeak, &stored.key).contains("blocked_leak"));
}
