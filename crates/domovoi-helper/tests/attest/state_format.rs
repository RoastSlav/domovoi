use domovoi_core::attest::{BootId, Hash32, ZERO_HASH};
use domovoi_core::host::KeyFingerprint;
use domovoi_helper::attest::compute::plan;
use domovoi_helper::attest::state::{STATE_FORMAT, StateFault, StateFile};
use proptest::prelude::*;

fn boot(n: u8) -> BootId {
    BootId::parse(&format!("5f0c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a{n:02x}")).expect("boot")
}

fn fingerprint() -> KeyFingerprint {
    Hash32::of(b"the key").into()
}

fn state(generation: u64, config: &[u8], routes: &[u8], boot_n: u8) -> StateFile {
    StateFile {
        key_fingerprint: fingerprint(),
        generation,
        config_hash: Hash32::of(config),
        route_state_hash: Hash32::of(routes),
        boot_id: boot(boot_n),
    }
}

#[test]
fn state_file_has_exact_format_checksum_and_modes() {
    let fresh = StateFile::fresh(fingerprint(), boot(1));
    let bytes = fresh.render();
    let text = String::from_utf8(bytes.clone()).expect("utf-8");
    let zero = ZERO_HASH.to_string();
    let body = format!(
        "format=1\nkey_fingerprint={}\ngeneration=0\nconfig_hash={zero}\nroute_state_hash={zero}\nboot_id={}\n",
        fingerprint(),
        boot(1)
    );
    assert_eq!(
        text,
        format!("{body}checksum={}\n", Hash32::of(body.as_bytes()))
    );
    assert_eq!(StateFile::parse(&bytes), Ok(fresh));
    assert_eq!(STATE_FORMAT, 1);
}

fn mutate(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    String::from_utf8_lossy(bytes)
        .replacen(from, to, 1)
        .into_bytes()
}

#[test]
fn the_state_parser_refuses_every_deviation() {
    let good = state(3, b"c", b"r", 1).render();
    assert!(StateFile::parse(&good).is_ok());
    let text = String::from_utf8(good.clone()).expect("utf-8");
    let swapped = {
        let mut lines: Vec<&str> = text.lines().collect();
        lines.swap(2, 3);
        format!("{}\n", lines.join("\n")).into_bytes()
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("no final LF", good[..good.len() - 1].to_vec()),
        ("extra line", [good.clone(), b"extra=1\n".to_vec()].concat()),
        ("CRLF", text.replace('\n', "\r\n").into_bytes()),
        ("other format", mutate(&good, "format=1", "format=2")),
        (
            "leading zero generation",
            mutate(&good, "generation=3", "generation=03"),
        ),
        ("uppercase hash", {
            let hash = Hash32::of(b"c").to_string();
            mutate(&good, &hash, &hash.to_uppercase())
        }),
        ("uppercase boot id", mutate(&good, "5f0c1a2b", "5F0C1A2B")),
        ("keys out of order", swapped),
        (
            "bad checksum",
            mutate(&good, "generation=3", "generation=4"),
        ),
        ("not utf-8", vec![0xff, 0xfe]),
    ];
    for (name, bytes) in cases {
        assert!(StateFile::parse(&bytes).is_err(), "{name} must be refused");
    }
    assert_eq!(
        StateFile::parse(&mutate(&good, "generation=3", "generation=4")),
        Err(StateFault::Checksum)
    );
}

#[test]
fn first_attestation_is_generation_one_after_init() {
    let init = StateFile::fresh(fingerprint(), boot(1));
    let next = plan(&init, Hash32::of(b"c"), Hash32::of(b"r"), &boot(1)).expect("plans");
    assert_eq!(next.generation, 1);
    let written = next.write.expect("the first attestation writes");
    assert_eq!(written.generation, 1);
    assert_eq!(written.config_hash, Hash32::of(b"c"));
}

#[test]
fn generation_never_decreases_under_same_key() {
    let committed = state(5, b"c", b"r", 1);
    for (config, routes) in [
        (&b"c"[..], &b"r"[..]),
        (b"c2", b"r"),
        (b"c", b"r2"),
        (b"c3", b"r3"),
    ] {
        let next =
            plan(&committed, Hash32::of(config), Hash32::of(routes), &boot(1)).expect("plans");
        assert!(next.generation >= committed.generation);
    }
    let same = plan(&committed, Hash32::of(b"c"), Hash32::of(b"r"), &boot(1)).expect("plans");
    assert_eq!(
        (same.generation, same.write),
        (5, None),
        "nothing changed: no write"
    );
    let changed = plan(&committed, Hash32::of(b"c2"), Hash32::of(b"r"), &boot(1)).expect("plans");
    assert_eq!(changed.generation, 6, "exactly one more");
}

#[test]
fn a_new_boot_id_alone_is_written_without_a_new_generation() {
    let committed = state(5, b"c", b"r", 1);
    let rebooted = plan(&committed, Hash32::of(b"c"), Hash32::of(b"r"), &boot(2)).expect("plans");
    assert_eq!(rebooted.generation, 5);
    assert_eq!(
        rebooted.write.expect("the boot id is recorded").boot_id,
        boot(2)
    );
}

#[test]
fn a_change_followed_by_its_revert_advances_the_generation_by_two() {
    let start = state(5, b"c", b"r", 1);
    let changed = plan(&start, Hash32::of(b"c2"), Hash32::of(b"r"), &boot(1)).expect("plans");
    let after_change = changed.write.expect("written");
    let reverted =
        plan(&after_change, Hash32::of(b"c"), Hash32::of(b"r"), &boot(1)).expect("plans");
    assert_eq!(reverted.generation, 7);
}

#[test]
fn containment_membership_change_keeps_generation() {
    // The generation function takes the two hashes and the boot id and nothing else, so a change
    // that is not one of the two hash inputs (such as the members of a containment set) cannot
    // reach it. The same inputs give the same generation, whatever happened in between.
    let committed = state(9, b"c", b"r", 1);
    for _ in 0..3 {
        let next = plan(
            &committed,
            committed.config_hash,
            committed.route_state_hash,
            &boot(1),
        )
        .expect("plans");
        assert_eq!(next.generation, 9);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn state_parser_round_trips(
        generation in any::<u64>(),
        config in any::<[u8; 32]>(),
        routes in any::<[u8; 32]>(),
        key in any::<[u8; 32]>(),
        boot_bytes in any::<[u8; 16]>(),
    ) {
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let h = hex(&boot_bytes);
        let boot_id = BootId::parse(&format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])).expect("boot");
        let original = StateFile {
            key_fingerprint: Hash32::from_bytes(key).into(),
            generation,
            config_hash: Hash32::from_bytes(config),
            route_state_hash: Hash32::from_bytes(routes),
            boot_id,
        };
        let bytes = original.render();
        prop_assert_eq!(StateFile::parse(&bytes), Ok(original.clone()));
        // A single flipped byte never parses to the same state.
        for index in [0, bytes.len() / 2, bytes.len() - 1] {
            let mut flipped = bytes.clone();
            flipped[index] ^= 0x01;
            prop_assert_ne!(StateFile::parse(&flipped).ok(), Some(original.clone()));
        }
    }

    #[test]
    fn generation_sequence_is_non_decreasing_under_interleaved_changes_and_restarts(
        steps in proptest::collection::vec((0_u8..3, any::<bool>()), 1..40)
    ) {
        let mut committed = StateFile::fresh(fingerprint(), boot(0));
        let mut last_seen = 0;
        let mut boot_n = 0;
        for (input, reboot) in steps {
            if reboot { boot_n += 1; }
            let config = Hash32::of(&[input]);
            let next = plan(&committed, config, committed.route_state_hash, &boot(boot_n)).expect("plans");
            prop_assert!(next.generation >= last_seen);
            if committed.config_hash != config {
                prop_assert_eq!(next.generation, last_seen + 1, "a change is exactly one more");
            }
            last_seen = next.generation;
            if let Some(write) = next.write { committed = write; }
        }
    }
}
